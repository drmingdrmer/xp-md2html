def render_to_img(mime, input, typ, width=1000, height=2000, asset_base=None):
    '''
    Render content that is renderable in chrome to image.
    Such as html, svg etc into image.
    It uses a headless chrome to render the page.
    Requirement: Chrome, imagemagick

    Args:
        mime(str): a full mime type such as ``image/jpeg`` or a shortcut ``jpg``.

        input(str): content of the input, such as jpeg data or svg source file.

        typ(string): specifies output image type such as "png", "jpg"

        width(int): specifies the window width to render a page. Default 1000.

        height(int): specifies the window height to render a page. Default 2000.

        asset_base(str): specifies the path to assets dir. E.g. the image base path in a html page.

    Returns:
        bytes of the png data
    '''

    if 'html' in mime:
        input = r'<meta http-equiv="Content-Type" content="text/html; charset=utf-8"/>' + input

        # Only for html page we need to add asset_base url.
        if asset_base is not None:
            input = r'<base href="file://{}/">'.format(asset_base) + input

    m = mimetypes.get(mime) or mime

    # Use the input mime type as temp page suffix.
    suffix = mime

    # If the input ``mime`` is a full mime type such as `application/xhtml+xml`,
    # convert it back to file suffix.
    for k, v in mimetypes.items():
        if v == m:
            suffix = k
            break

    chrome = 'google-chrome'
    if sys.platform == 'darwin':
        # mac
        chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

    with tempfile.TemporaryDirectory() as tdir:
        # Write page content into a temp file.
        # Since chrome does not recoganize the `<base>` tag encoded in a
        # data-uri.
        fn = os.path.join(tdir, 'xxx.' + suffix)
        flags = 'w'
        if isinstance(input, bytes):
            flags = 'wb'
        with open(fn, flags) as f:
            f.write(input)

        k3proc.command_ex(
            chrome,
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
            "--screenshot",
            "--window-size={},{}".format(width, height),
            "--default-background-color=00000000",
            fn,
            cwd=tdir,
        )

        if typ == 'png':
            moreargs = []
        else:
            # flatten alpha channel
            moreargs = ['-background', 'white', '-flatten', '-alpha', 'off']

        # crop to visible area
        _, out, _ = k3proc.command_ex(
            "convert",
            pjoin(tdir, "screenshot.png"),
            "-trim",
            "+repage",
            *moreargs,
            typ + ":-",
            text=False,
            )

    return out
