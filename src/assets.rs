//! Static files compiled into the binary. The rig serves no static files, and the
//! deployment host is a LAN box that is not assumed to reach a CDN, so the CSS and
//! both JS libraries ship inside the executable.

pub struct Asset {
    pub mime: &'static str,
    pub body: &'static [u8],
}

/// Look up an asset by its path relative to `/static/`.
pub fn get(path: &str) -> Option<Asset> {
    let (mime, body): (&'static str, &'static [u8]) = match path {
        "css/spooky.css" => ("text/css", include_bytes!("../static/css/spooky.css")),
        "js/app.js" => ("text/javascript", include_bytes!("../static/js/app.js")),
        "vendor/htmx.min.js" => (
            "text/javascript",
            include_bytes!("../static/vendor/htmx.min.js"),
        ),
        "vendor/chart.umd.min.js" => (
            "text/javascript",
            include_bytes!("../static/vendor/chart.umd.min.js"),
        ),
        _ => return None,
    };
    Some(Asset { mime, body })
}
