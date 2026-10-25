use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Args;

use super::ProcessArgs;

#[derive(Args)]
pub(super) struct BatchArgs {
    #[command(flatten)]
    options: ProcessArgs,

    /// Input files; use -o 'out/{title}.md' or -o out/ for multiple files
    #[arg(value_name = "INPUT", conflicts_with = "input")]
    inputs: Vec<PathBuf>,

    /// Preserve front matter and dated filenames when writing to an output directory
    #[arg(long)]
    jekyll: bool,
}

#[derive(Hash, PartialEq, Eq)]
enum FileIdentity {
    Existing(same_file::Handle),
    Missing(PathBuf),
}

pub(super) fn process_command(args: BatchArgs) -> Result<()> {
    let jobs = prepare(args).map_err(|error| {
        clap::Error::raw(
            clap::error::ErrorKind::ValueValidation,
            format!("{error:#}\n"),
        )
    })?;
    for job in jobs {
        super::process_command(job)?;
    }
    Ok(())
}

fn prepare(mut args: BatchArgs) -> Result<Vec<ProcessArgs>> {
    if let Some(input) = args.options.input.take() {
        args.inputs.push(input);
    }
    if args.inputs.len() > 1 && args.options.output.is_none() {
        anyhow::bail!("Multiple inputs require --output with {{title}} or a trailing /");
    }
    if args.jekyll && args.inputs.is_empty() {
        anyhow::bail!("--jekyll requires an input file");
    }
    let mut jobs = Vec::new();
    if args.inputs.is_empty() {
        jobs.push(expand_job(&args, None)?);
    } else {
        for input in &args.inputs {
            jobs.push(expand_job(&args, Some(input))?);
        }
    }
    validate_jobs(&jobs)?;
    Ok(jobs)
}

fn article_name(path: &Path) -> Result<&str> {
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    let stem = stem.with_context(|| format!("Input has no UTF-8 file stem: {}", path.display()))?;
    let title = trim_date_prefix(stem);
    anyhow::ensure!(
        !title.is_empty(),
        "Input has an empty title: {}",
        path.display()
    );
    Ok(title)
}

fn trim_date_prefix(name: &str) -> &str {
    const PATTERN: &[u8] = b"0000-00-00-";
    let Some(prefix) = name.as_bytes().get(..PATTERN.len()) else {
        return name;
    };
    let dated = prefix.iter().zip(PATTERN).all(|(byte, pattern)| {
        if *pattern == b'-' {
            *byte == b'-'
        } else {
            byte.is_ascii_digit()
        }
    });
    if dated {
        &name[PATTERN.len()..]
    } else {
        name
    }
}

fn expand_template(template: &str, title: Option<&str>) -> Result<(String, bool)> {
    let mut templated = false;
    let mut result = String::new();
    let mut rest = template;
    while !rest.is_empty() {
        if rest.starts_with("{{") || rest.starts_with("}}") {
            result.push(rest.as_bytes()[0] as char);
            rest = &rest[2..];
        } else if let Some(tail) = rest.strip_prefix("{title}") {
            templated = true;
            result.push_str(title.context("{title} requires an input file")?);
            rest = tail;
        } else {
            let character = rest.chars().next().context("Empty template")?;
            anyhow::ensure!(character != '{' && character != '}', "Invalid template {template:?}: the only placeholder is {{title}}; double literal braces as {{{{ or }}}}");
            result.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    Ok((result, templated))
}

fn expand_path(path: &Path, title: Option<&str>) -> Result<(PathBuf, bool)> {
    let text = path.to_str().context("Template paths must be UTF-8")?;
    let (expanded, templated) = expand_template(text, title)?;
    Ok((PathBuf::from(expanded), templated))
}

fn expand_output(output: &Path, input: Option<&Path>, jekyll: bool) -> Result<(PathBuf, bool)> {
    let title = input.map(article_name).transpose()?;
    let (mut expanded, templated) = expand_path(output, title)?;
    let directory = output.as_os_str().as_encoded_bytes().ends_with(b"/");
    if directory {
        let input = input.context("An output directory requires an input file")?;
        let name = input.file_name().and_then(|name| name.to_str());
        let name = name.context("Input has no UTF-8 filename")?;
        let name = if jekyll { name } else { trim_date_prefix(name) };
        expanded.push(name);
    }
    super::validate_output(&expanded)?;
    Ok((expanded, templated || directory))
}

fn expand_job(args: &BatchArgs, input: Option<&PathBuf>) -> Result<ProcessArgs> {
    let mut job = args.options.clone();
    job.input = input.cloned();
    job.keep_front_matter |= args.jekyll;
    let input = input.map(PathBuf::as_path);
    let title = input.map(article_name).transpose()?;
    if let Some(output) = &args.options.output {
        let (expanded, templated) = expand_output(output, input, args.jekyll)?;
        let per_article = args.inputs.len() > 1 || templated || args.jekyll;
        if job.assets.is_none() && per_article {
            let parent = expanded.parent().context("Output has no directory")?;
            job.assets =
                Some(parent.join(title.context("Per-article assets require an input file")?));
        }
        job.output = Some(expanded);
    }
    if let Some(assets) = &args.options.assets {
        let (expanded, _) = expand_path(assets, title)?;
        job.assets = Some(expanded);
    }
    if let Some(url) = &args.options.url_base {
        let (expanded, _) = expand_template(url, title)?;
        job.url_base = Some(expanded);
    }
    Ok(job)
}

// Resolve symlinks before processing .., including parents of files not created yet.
fn resolved_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            component => {
                resolved.push(component.as_os_str());
                match fs::canonicalize(&resolved) {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(resolved)
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let resolved = resolved_path(path)?;
    match same_file::Handle::from_path(&resolved) {
        Ok(handle) => Ok(FileIdentity::Existing(handle)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(FileIdentity::Missing(resolved))
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_jobs(jobs: &[ProcessArgs]) -> Result<()> {
    let mut inputs = HashMap::new();
    for (index, job) in jobs.iter().enumerate() {
        if let Some(input) = &job.input {
            if jobs.len() > 1 {
                let metadata = fs::metadata(input)
                    .with_context(|| format!("Invalid input file: {}", input.display()))?;
                anyhow::ensure!(
                    metadata.is_file(),
                    "Input is not a file: {}",
                    input.display()
                );
            }
            let identity = file_identity(input)?;
            inputs.entry(identity).or_insert_with(Vec::new).push(index);
        }
    }
    validate_directories(jobs)?;
    let mut outputs = HashMap::new();
    for (index, job) in jobs.iter().enumerate() {
        let Some(output) = &job.output else { continue };
        let identity = file_identity(output)?;
        if let Some(indices) = inputs.get(&identity) {
            let other_input = indices.iter().any(|other| *other != index);
            anyhow::ensure!(
                !other_input,
                "Output aliases another input: {}",
                output.display()
            );
        }
        let previous = outputs.insert(identity, index);
        anyhow::ensure!(previous.is_none(), "Duplicate output: {}", output.display());
    }
    Ok(())
}

fn validate_directories(jobs: &[ProcessArgs]) -> Result<()> {
    let mut destinations = HashSet::new();
    for job in jobs {
        if let Some(output) = &job.output {
            destinations.insert(resolved_path(output)?);
        }
    }
    for job in jobs {
        validate_url_base(job)?;
        if let Some(output) = &job.output {
            validate_directory(
                output.parent().context("Output has no parent")?,
                &destinations,
            )?;
        }
        if let Some(assets) = &job.assets {
            validate_directory(assets, &destinations)?;
        }
    }
    Ok(())
}

fn validate_directory(directory: &Path, destinations: &HashSet<PathBuf>) -> Result<()> {
    for parent in directory.ancestors() {
        if parent.as_os_str().is_empty() {
            continue;
        }
        let resolved = resolved_path(parent)?;
        anyhow::ensure!(
            !destinations.contains(&resolved),
            "Output also used as a directory: {}",
            parent.display()
        );
        let metadata = super::output_metadata(parent)?;
        if let Some(metadata) = metadata {
            anyhow::ensure!(metadata.is_dir(), "Not a directory: {}", parent.display());
        }
    }
    Ok(())
}

fn validate_url_base(job: &ProcessArgs) -> Result<()> {
    if job.url_base.is_none() {
        return Ok(());
    }
    let output_dir = super::file_dir(job.output.as_deref())?;
    let assets_dir = job.assets.as_deref().unwrap_or(&output_dir);
    let assets_path = xp_md2html::process::relative_url(&output_dir, assets_dir)?;
    let first_part = assets_path.split('/').next();
    anyhow::ensure!(
        first_part != Some(".."),
        "With --url-base, --assets must be inside the output file's directory {}: {}",
        output_dir.display(),
        assets_dir.display()
    );
    Ok(())
}
