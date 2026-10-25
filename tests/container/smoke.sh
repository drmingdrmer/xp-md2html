#!/bin/sh
set -eu

mkdir -p "$1"
cd "$1"
cat > smoke.md <<'MARKDOWN'
$$
E = mc^2
$$

```rust
fn main() { println!("中文"); }
```

| 中文 | English |
| --- | --- |
| 表格 | Table |

```mermaid
graph LR
  A[开始] --> B[结束]
```

```graphviz
digraph { 开始 -> 结束 }
```
MARKDOWN

fc-match -f '%{family}' 'sans-serif:lang=zh-cn' | grep -F 'Noto Sans CJK'
xpmd process --preset simple --scale 1 -i smoke.md -o rendered.md --assets assets
: > expected.md
for kind in math code table mermaid graphviz; do
    set -- assets/rendered-"$kind"-*.png
    test "$#" -eq 1
    test -f "$1"
    printf '![](%s)\n\n' "$1" >> expected.md
    stats=$(magick identify -format '%m %w %h %k' "$1")
    # Split the four numeric/image-format fields printed by identify.
    # shellcheck disable=SC2086
    set -- $stats
    test "$1" = PNG
    test "$2" -gt 1
    test "$3" -gt 1
    test "$4" -gt 1
done
# CommonMark output ends with one newline, rather than an empty paragraph.
sed '$d' expected.md > expected-final.md
cmp expected-final.md rendered.md
set -- assets/*
test "$#" -eq 5
