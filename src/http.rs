use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    ops::Range,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 128 * 1024;
const MAX_IMAGE_BYTES: usize = 512 * 1024;
const MAX_PROFILE_BODY: usize = MAX_IMAGE_BYTES + 8 * 1024;
const MAX_COMMUNITY_BODY: usize = 2 * MAX_IMAGE_BYTES + 32 * 1024;
const MAX_POST_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_POST_VIDEO_BYTES: usize = 16 * 1024 * 1024;
const MAX_POST_BODY: usize = MAX_POST_VIDEO_BYTES + 100 * 1024;
const MAX_POST_FILES: usize = 6;
const MAX_POST_FIELD_BYTES: usize = 64 * 1024;
const MAX_PART_HEADERS: usize = 4 * 1024;
const MAX_WIZARD_FIELD_BYTES: usize = 8 * 1024;
const MAX_CONNECTIONS: usize = 16;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const SESSION_COOKIE_NAME: &str = "commonline_session";
pub(crate) const SESSION_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;
const WIZARD_FIELDS: &[&str] = &[
    "review_community",
    "setup_step",
    "topic",
    "visibility",
    "mature",
    "slug",
    "name",
    "description",
    "guidelines",
    "artwork_preview",
    "remove_icon",
    "remove_banner",
];
const ARTWORK_SETTINGS_FIELDS: &[&str] = &[
    "community",
    "artwork_preview",
    "remove_icon",
    "remove_banner",
];
const POST_MEDIA_FIELDS: &[&str] = &[
    "community",
    "title",
    "body",
    "flair",
    "sensitive",
    "spoiler",
    "brand_affiliate",
    "url",
];

type Fields = BTreeMap<String, String>;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Fields,
    pub form: Fields,
    pub origin: String,
    pub upload: Option<Upload>,
    pub community_uploads: BTreeMap<String, Upload>,
    pub post_uploads: Vec<Upload>,
    pub range: Option<String>,
    pub cookie: Option<String>,
    pub user_agent: Option<String>,
}

pub struct Upload {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

enum UploadTarget {
    Profile,
    Community(&'static [&'static str]),
    PostMedia,
}

pub struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    location: Option<String>,
    media_length: Option<usize>,
    body_slice: Option<Range<usize>>,
    session_cookie: Option<String>,
}

impl Response {
    pub fn html(body: String) -> Self {
        Self::text(200, "text/html; charset=utf-8", body)
    }

    pub fn redirect(location: &str) -> Self {
        if !location.starts_with('/')
            || location.starts_with("//")
            || location.contains('\\')
            || !location.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Self::error(500, "Invalid redirect destination.");
        }
        Self {
            location: Some(location.to_owned()),
            ..Self::text(303, "text/plain; charset=utf-8", "See other")
        }
    }

    pub fn not_found(body: String) -> Self {
        Self::text(404, "text/html; charset=utf-8", body)
    }

    pub fn text(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type,
            body: body.into(),
            location: None,
            media_length: None,
            body_slice: None,
            session_cookie: None,
        }
    }

    pub fn with_session_cookie(mut self, token: Option<&str>) -> Self {
        let (value, max_age) = match token {
            Some(value) if valid_session_token(value) => (value, SESSION_MAX_AGE_SECS),
            Some(_) => return self,
            None => ("", 0),
        };
        self.session_cookie = Some(format!(
            "{SESSION_COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}"
        ));
        self
    }

    pub fn media(content_type: &'static str, bytes: Vec<u8>, range: Option<&str>) -> Self {
        let length = bytes.len();
        let mut response = Self::text(200, content_type, bytes);
        response.media_length = Some(length);
        let Some(header) = range else {
            return response;
        };
        let selection = match header.split_once('=') {
            Some((unit, value)) if unit.eq_ignore_ascii_case("bytes") => byte_range(value, length),
            Some(_) => return response,
            None => None,
        };
        if let Some(selection) = selection {
            response.status = 206;
            response.body_slice = Some(selection);
        } else {
            response.status = 416;
            response.body = Vec::new();
        }
        response
    }

    fn error(status: u16, message: &'static str) -> Self {
        Self::text(status, "text/plain; charset=utf-8", message)
    }

    fn write(&self, stream: &mut TcpStream, head_only: bool) -> io::Result<()> {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            206 => "Partial Content",
            303 => "See Other",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            413 => "Content Too Large",
            414 => "URI Too Long",
            415 => "Unsupported Media Type",
            416 => "Range Not Satisfiable",
            417 => "Expectation Failed",
            422 => "Unprocessable Content",
            431 => "Request Header Fields Too Large",
            503 => "Service Unavailable",
            _ => "Internal Server Error",
        };
        let body = match &self.body_slice {
            Some(selection) => &self.body[selection.clone()],
            None => self.body.as_slice(),
        };
        let mut headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
             Connection: close\r\nCache-Control: no-store\r\n\
             X-Content-Type-Options: nosniff\r\nReferrer-Policy: same-origin\r\n\
             Content-Security-Policy: default-src 'none'; style-src 'self'; img-src 'self' data:; media-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'\r\n",
            self.status,
            reason,
            self.content_type,
            body.len()
        );
        if let Some(location) = &self.location {
            headers.push_str(&format!("Location: {location}\r\n"));
        }
        if let Some(cookie) = &self.session_cookie {
            headers.push_str(&format!("Set-Cookie: {cookie}\r\n"));
        }
        if self.status == 405 {
            headers.push_str("Allow: GET, HEAD, POST\r\n");
        }
        if let Some(length) = self.media_length {
            headers.push_str("Accept-Ranges: bytes\r\n");
            if let Some(selection) = &self.body_slice {
                headers.push_str(&format!(
                    "Content-Range: bytes {}-{}/{length}\r\n",
                    selection.start,
                    selection.end - 1,
                ));
            } else if self.status == 416 {
                headers.push_str(&format!("Content-Range: bytes */{length}\r\n"));
            }
        }
        headers.push_str("\r\n");
        stream.write_all(headers.as_bytes())?;
        if !head_only {
            stream.write_all(body)?;
        }
        stream.flush()
    }
}

pub(crate) fn valid_session_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn byte_range(value: &str, length: usize) -> Option<Range<usize>> {
    if length == 0 {
        return None;
    }
    let (first, last) = value.trim().split_once('-')?;
    if first.is_empty() {
        let suffix = range_number(last)?;
        return (suffix > 0).then_some(length.saturating_sub(suffix)..length);
    }
    let start = range_number(first)?;
    let end = if last.is_empty() {
        length
    } else {
        range_number(last)?.saturating_add(1).min(length)
    };
    (start < end).then_some(start..end)
}

fn range_number(value: &str) -> Option<usize> {
    if value.is_empty() {
        return None;
    }
    value.bytes().try_fold(0_usize, |number, byte| {
        byte.is_ascii_digit().then(|| {
            number
                .saturating_mul(10)
                .saturating_add(usize::from(byte - b'0'))
        })
    })
}

pub fn serve<F>(port: u16, render: F) -> io::Result<()>
where
    F: FnMut(Request) -> Response + Send + 'static,
{
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let address = listener.local_addr()?;
    println!("Commonline is running at http://{address}/");
    let mut stdout = io::stdout().lock();
    stdout.flush()?;
    drop(stdout);

    let active = Arc::new(AtomicUsize::new(0));
    let render = Arc::new(Mutex::new(render));
    for connection in listener.incoming() {
        let mut stream = match connection {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let slot = ConnectionSlot::new(Arc::clone(&active));
        if slot.exceeds_limit() {
            let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
            let _ = Response::error(503, "Too many active connections.").write(&mut stream, false);
            continue;
        }
        let render = Arc::clone(&render);
        thread::Builder::new()
            .name("commonline-http".to_owned())
            .spawn(move || {
                let _slot = slot;
                let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
                let mut head_only = false;
                let (response, rejected_request) =
                    match read_request(&mut stream, address.port(), &mut head_only) {
                        Ok(request) => {
                            let response = match render.lock() {
                                Ok(mut render) => render(request),
                                Err(_) => Response::error(500, "The local state is unavailable."),
                            };
                            (response, false)
                        }
                        Err(response) => (response, true),
                    };
                let _ = response.write(&mut stream, head_only);
                if rejected_request {
                    finish_rejected_body(&mut stream);
                }
                let _ = stream.shutdown(Shutdown::Write);
            })?;
    }
    Ok(())
}

struct ConnectionSlot {
    active: Arc<AtomicUsize>,
    count: usize,
}

impl ConnectionSlot {
    fn new(active: Arc<AtomicUsize>) -> Self {
        let count = active.fetch_add(1, Ordering::Relaxed) + 1;
        Self { active, count }
    }

    fn exceeds_limit(&self) -> bool {
        self.count > MAX_CONNECTIONS
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::Relaxed);
    }
}

fn read_request(
    stream: &mut TcpStream,
    port: u16,
    head_only: &mut bool,
) -> Result<Request, Response> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut received = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let head_end = loop {
        if let Some(end) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            if end <= MAX_HEADERS {
                break end;
            }
        }
        if received.len() >= MAX_HEADERS + 4 {
            return Err(Response::error(431, "The request headers are too large."));
        }
        let count = read_limited(stream, &mut chunk, deadline)?;
        received.extend_from_slice(&chunk[..count]);
    };

    let head = std::str::from_utf8(&received[..head_end])
        .map_err(|_| Response::error(400, "Invalid request headers."))?;
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or_default().split(' ');
    let (method, target, version) = match (start.next(), start.next(), start.next(), start.next()) {
        (Some(method), Some(target), Some(version), None) => (method, target, version),
        _ => return Err(Response::error(400, "Invalid request line.")),
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(Response::error(
            400,
            "Only HTTP/1.0 and HTTP/1.1 are supported.",
        ));
    }
    *head_only = method == "HEAD";
    if !matches!(method, "GET" | "HEAD" | "POST") {
        return Err(Response::error(405, "This HTTP method is not supported."));
    }
    if target.len() > 4096 {
        return Err(Response::error(414, "The request URL is too long."));
    }
    if !target.starts_with('/')
        || target.starts_with("//")
        || target.contains(['#', '\\'])
        || !target.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(Response::error(400, "Invalid request URL."));
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request {
        method: if *head_only { "GET" } else { method }.to_owned(),
        path: path.to_owned(),
        query: parse_fields(query)?,
        form: Fields::new(),
        origin: format!("http://127.0.0.1:{port}"),
        upload: None,
        community_uploads: BTreeMap::new(),
        post_uploads: Vec::new(),
        range: None,
        cookie: None,
        user_agent: None,
    };
    let headers = parse_headers(lines)?;
    request.cookie = headers.get("cookie").cloned();
    request.user_agent = headers.get("user-agent").cloned();
    if let Some(range) = headers.get("range") {
        if !range
            .bytes()
            .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
        {
            return Err(Response::error(400, "Invalid byte-range header."));
        }
        if method == "GET" && !headers.contains_key("if-range") {
            request.range = Some(range.clone());
        }
    }
    let host = headers.get("host").map(String::as_str);
    if !valid_host(host, port, version) {
        return Err(Response::error(
            403,
            "Use this app at its loopback address.",
        ));
    }
    if headers.contains_key("transfer-encoding") {
        return Err(Response::error(
            400,
            "Streaming request bodies are unsupported.",
        ));
    }
    if headers.contains_key("expect") {
        return Err(Response::error(
            417,
            "Request expectations are unsupported.",
        ));
    }

    let upload_target = match (method, path) {
        ("POST", "/profile/image") => Some(UploadTarget::Profile),
        ("POST", "/communities/appearance") => Some(UploadTarget::Community(WIZARD_FIELDS)),
        ("POST", "/communities/appearance/settings") => {
            Some(UploadTarget::Community(ARTWORK_SETTINGS_FIELDS))
        }
        ("POST", "/posts/media/new") => Some(UploadTarget::PostMedia),
        _ => None,
    };
    let max_body = match upload_target {
        Some(UploadTarget::Profile) => MAX_PROFILE_BODY,
        Some(UploadTarget::Community(_)) => MAX_COMMUNITY_BODY,
        Some(UploadTarget::PostMedia) => MAX_POST_BODY,
        None => MAX_BODY,
    };
    let length = content_length(headers.get("content-length"), max_body)?;
    if method != "POST" {
        if length > 0 {
            return Err(Response::error(400, "Unexpected request body."));
        }
        return Ok(request);
    }

    check_post_origin(&headers, host)?;
    let media_type = headers.get("content-type").map_or("", String::as_str);
    let format = media_type.split(';').next().unwrap_or_default().trim();
    let multipart = if let Some(target) = upload_target {
        if !format.eq_ignore_ascii_case("multipart/form-data") {
            let description = match target {
                UploadTarget::PostMedia => "Expected a multipart media form.",
                _ => "Expected a multipart image form.",
            };
            return Err(Response::error(415, description));
        }
        Some((target, upload_boundary(media_type)?))
    } else {
        if !format.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
            return Err(Response::error(415, "Expected a URL-encoded form."));
        }
        None
    };
    let mut body = received.split_off(head_end + 4);
    body.truncate(length);
    let mut filled = body.len();
    body.resize(length, 0);
    while filled < length {
        filled += read_limited(stream, &mut body[filled..], deadline)?;
    }
    match multipart {
        Some((UploadTarget::Profile, boundary)) => {
            let (kind, upload) = parse_upload(&body, &boundary)?;
            request.form.insert("kind".to_owned(), kind);
            request.upload = Some(upload);
        }
        Some((UploadTarget::Community(fields), boundary)) => {
            (request.form, request.community_uploads) =
                parse_community_uploads(&body, &boundary, fields)?;
        }
        Some((UploadTarget::PostMedia, boundary)) => {
            (request.form, request.post_uploads) = parse_post_uploads(body, &boundary)?;
        }
        None => {
            let body = std::str::from_utf8(&body)
                .map_err(|_| Response::error(400, "The form must use UTF-8."))?;
            request.form = parse_fields(body)?;
        }
    }
    Ok(request)
}

fn read_limited(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, Response> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(Response::error(408, "Request timed out."));
    }
    stream
        .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
        .map_err(|_| Response::error(500, "Could not set the connection timeout."))?;
    match stream.read(buffer) {
        Ok(0) => Err(Response::error(400, "The request ended early.")),
        Ok(count) => Ok(count),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            Err(Response::error(408, "Request timed out."))
        }
        Err(_) => Err(Response::error(400, "Could not read the request.")),
    }
}

fn finish_rejected_body(stream: &mut TcpStream) {
    // Avoid resetting an in-progress sender before it receives the rejection.
    let deadline = Instant::now() + Duration::from_millis(150);
    let mut remaining = 2 * MAX_COMMUNITY_BODY;
    let mut discard = [0_u8; 8192];
    while remaining > 0 {
        let length = remaining.min(discard.len());
        let Ok(read) = read_limited(stream, &mut discard[..length], deadline) else {
            break;
        };
        remaining -= read;
    }
}

fn parse_headers<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Fields, Response> {
    let mut headers = Fields::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| Response::error(400, "Invalid request header."))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value
                .bytes()
                .any(|byte| (byte < 32 && byte != b'\t') || byte == 127)
        {
            return Err(Response::error(400, "Invalid request header."));
        }
        let name = name.to_ascii_lowercase();
        if headers.contains_key(&name)
            && matches!(
                name.as_str(),
                "host"
                    | "content-length"
                    | "content-type"
                    | "content-disposition"
                    | "origin"
                    | "sec-fetch-site"
                    | "range"
                    | "cookie"
            )
        {
            return Err(Response::error(400, "Duplicate request header."));
        }
        headers.insert(name, value.trim_matches([' ', '\t']).to_owned());
    }
    Ok(headers)
}

fn valid_host(host: Option<&str>, port: u16, version: &str) -> bool {
    match host {
        Some(host) => {
            let (name, actual_port) = host.split_once(':').unwrap_or((host, "80"));
            (name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1")
                && actual_port.parse::<u16>() == Ok(port)
        }
        None => version == "HTTP/1.0",
    }
}

fn content_length(value: Option<&String>, limit: usize) -> Result<usize, Response> {
    let Some(value) = value else {
        return Ok(0);
    };
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Response::error(400, "Invalid content length."));
    }
    match value.parse::<usize>() {
        Ok(length) if length <= limit => Ok(length),
        _ => Err(Response::error(413, "The request body is too large.")),
    }
}

fn check_post_origin(headers: &Fields, host: Option<&str>) -> Result<(), Response> {
    let cross_origin = headers.get("origin").is_some_and(|origin| {
        !origin
            .strip_prefix("http://")
            .zip(host)
            .is_some_and(|(origin_host, request_host)| {
                origin_host.eq_ignore_ascii_case(request_host)
            })
    });
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|site| !matches!(site.as_str(), "same-origin" | "none"));
    if cross_origin || cross_site {
        return Err(Response::error(403, "Submit forms from Commonline."));
    }
    Ok(())
}

fn invalid_upload() -> Response {
    Response::error(400, "Select one image for your avatar or cover.")
}

fn invalid_community_upload() -> Response {
    Response::error(400, "Check the community artwork and form fields.")
}

fn upload_boundary(content_type: &str) -> Result<String, Response> {
    let bad_boundary = || Response::error(400, "Invalid multipart form boundary.");
    let (_, mut parameters) = parameterized_header(content_type).ok_or_else(bad_boundary)?;
    let boundary = parameters.remove("boundary").ok_or_else(bad_boundary)?;
    if boundary.is_empty()
        || boundary.len() > 70
        || boundary.ends_with(' ')
        || !boundary
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"'()+_,-./:=? ".contains(&byte))
    {
        return Err(bad_boundary());
    }
    Ok(boundary)
}

fn parse_upload(body: &[u8], boundary: &str) -> Result<(String, Upload), Response> {
    let mut kind = None;
    let mut upload = None;
    read_multipart(body, boundary, 2, invalid_upload, |part| {
        match (part.name.as_str(), part.filename.as_deref()) {
            ("kind", None) if kind.is_none() => {
                kind = Some(match part.data {
                    b"avatar" => "avatar".to_owned(),
                    b"cover" => "cover".to_owned(),
                    _ => return Err(invalid_upload()),
                });
            }
            ("image", Some(name))
                if upload.is_none() && !name.is_empty() && !part.data.is_empty() =>
            {
                upload = Some(read_image(part.content_type, part.data)?);
            }
            _ => return Err(invalid_upload()),
        }
        Ok(())
    })?;
    kind.zip(upload).ok_or_else(invalid_upload)
}

fn parse_community_uploads(
    body: &[u8],
    boundary: &str,
    allowed_fields: &[&str],
) -> Result<(Fields, BTreeMap<String, Upload>), Response> {
    let mut fields = Fields::new();
    let mut uploads = BTreeMap::new();
    let mut seen = BTreeSet::new();
    read_multipart(
        body,
        boundary,
        allowed_fields.len() + 2,
        invalid_community_upload,
        |part| {
            if !seen.insert(part.name.clone()) {
                return Err(invalid_community_upload());
            }
            match (part.name.as_str(), part.filename.as_deref()) {
                ("icon_image" | "banner_image", Some("")) if part.data.is_empty() => {}
                ("icon_image" | "banner_image", Some(name))
                    if !name.is_empty() && !part.data.is_empty() =>
                {
                    if !matches!(
                        part.content_type.as_str(),
                        "image/png" | "image/jpeg" | "image/webp"
                    ) {
                        return Err(Response::error(415, "Choose a PNG, JPEG, or WebP image."));
                    }
                    uploads.insert(part.name, read_image(part.content_type, part.data)?);
                }
                (name, None) if allowed_fields.contains(&name) => {
                    if part.data.len() > MAX_WIZARD_FIELD_BYTES {
                        return Err(Response::error(
                            413,
                            "A community setup field is too large.",
                        ));
                    }
                    let value = std::str::from_utf8(part.data)
                        .map_err(|_| Response::error(400, "The form must use UTF-8."))?;
                    fields.insert(part.name, value.to_owned());
                }
                _ => return Err(invalid_community_upload()),
            }
            Ok(())
        },
    )?;
    Ok((fields, uploads))
}

fn parse_post_uploads(
    mut body: Vec<u8>,
    boundary: &str,
) -> Result<(Fields, Vec<Upload>), Response> {
    let mut fields = Fields::new();
    let mut files = Vec::new();
    let mut seen_video = false;
    let invalid = || Response::error(400, "Check the post media and form fields.");
    read_multipart(
        &body,
        boundary,
        POST_MEDIA_FIELDS.len() + MAX_POST_FILES,
        invalid,
        |part| {
            match (part.name.as_str(), part.filename.as_deref()) {
                ("post_media", Some("")) if part.data.is_empty() => {}
                ("post_media", Some(name)) if !name.is_empty() && !part.data.is_empty() => {
                    let video = match part.content_type.as_str() {
                        "image/png" | "image/jpeg" | "image/webp" | "image/gif" => false,
                        "video/mp4" | "video/webm" => true,
                        _ => {
                            return Err(Response::error(
                                415,
                                "Choose a PNG, JPEG, WebP, GIF, MP4, or WebM file.",
                            ))
                        }
                    };
                    if seen_video || (video && !files.is_empty()) {
                        return Err(Response::error(
                            400,
                            "Choose a single video or up to six images.",
                        ));
                    }
                    if files.len() == MAX_POST_FILES {
                        return Err(Response::error(400, "Choose up to six images."));
                    }
                    let max = if video {
                        MAX_POST_VIDEO_BYTES
                    } else {
                        MAX_POST_IMAGE_BYTES
                    };
                    if part.data.len() > max {
                        let message = if video {
                            "Choose a video no larger than 16 MiB."
                        } else {
                            "Choose images no larger than 2 MiB each."
                        };
                        return Err(Response::error(413, message));
                    }
                    seen_video = video;
                    files.push((
                        part.content_type,
                        part.offset..part.offset + part.data.len(),
                    ));
                }
                (name, None) if POST_MEDIA_FIELDS.contains(&name) && !fields.contains_key(name) => {
                    if part.data.len() > MAX_POST_FIELD_BYTES {
                        return Err(Response::error(413, "A post field is too large."));
                    }
                    let value = std::str::from_utf8(part.data)
                        .map_err(|_| Response::error(400, "The form must use UTF-8."))?;
                    fields.insert(part.name, value.to_owned());
                }
                _ => return Err(invalid()),
            }
            Ok(())
        },
    )?;

    let uploads = if files.len() == 1 {
        // Reuse the original request allocation for a single image or large video.
        let (content_type, data) = files.pop().unwrap();
        let size = data.len();
        body.copy_within(data, 0);
        body.truncate(size);
        vec![Upload {
            content_type,
            bytes: body,
        }]
    } else {
        files
            .into_iter()
            .map(|(content_type, data)| Upload {
                content_type,
                bytes: body[data].to_vec(),
            })
            .collect()
    };
    Ok((fields, uploads))
}

fn read_image(content_type: String, data: &[u8]) -> Result<Upload, Response> {
    if data.len() > MAX_IMAGE_BYTES {
        return Err(Response::error(413, "Choose an image up to 512 KiB."));
    }
    Ok(Upload {
        content_type,
        bytes: data.to_vec(),
    })
}

struct MultipartPart<'a> {
    name: String,
    filename: Option<String>,
    content_type: String,
    data: &'a [u8],
    offset: usize,
}

fn read_multipart<'a>(
    body: &'a [u8],
    boundary: &str,
    max_parts: usize,
    invalid: fn() -> Response,
    mut visit: impl FnMut(MultipartPart<'a>) -> Result<(), Response>,
) -> Result<(), Response> {
    let opening = format!("--{boundary}\r\n");
    let delimiter = format!("\r\n--{boundary}");
    let mut remaining = body.strip_prefix(opening.as_bytes()).ok_or_else(invalid)?;

    for _ in 0..max_parts {
        let header_limit = remaining.len().min(MAX_PART_HEADERS + 4);
        let header_end = remaining[..header_limit]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(invalid)?;
        let headers = std::str::from_utf8(&remaining[..header_end]).map_err(|_| invalid())?;
        let headers = parse_headers(headers.split("\r\n"))?;
        if headers.contains_key("content-transfer-encoding") {
            return Err(invalid());
        }
        let contents = &remaining[header_end + 4..];
        let (data, suffix) = split_part(contents, delimiter.as_bytes()).ok_or_else(invalid)?;
        let (disposition, mut properties) = headers
            .get("content-disposition")
            .and_then(|header| parameterized_header(header))
            .ok_or_else(invalid)?;
        if !disposition.eq_ignore_ascii_case("form-data") {
            return Err(invalid());
        }
        let name = properties.remove("name").ok_or_else(invalid)?;
        let content_type = headers
            .get("content-type")
            .map(|header| header.split(';').next().unwrap_or_default().trim())
            .unwrap_or("application/octet-stream");
        visit(MultipartPart {
            name,
            filename: properties.remove("filename"),
            content_type: content_type.to_ascii_lowercase(),
            data,
            offset: body.len() - contents.len(),
        })?;
        if matches!(suffix, b"--" | b"--\r\n") {
            return Ok(());
        }
        remaining = suffix.strip_prefix(b"\r\n").ok_or_else(invalid)?;
    }
    Err(invalid())
}

fn split_part<'a>(body: &'a [u8], delimiter: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    for (index, window) in body.windows(delimiter.len()).enumerate() {
        if window == delimiter {
            let suffix = &body[index + delimiter.len()..];
            if suffix.starts_with(b"\r\n") || matches!(suffix, b"--" | b"--\r\n") {
                return Some((&body[..index], suffix));
            }
        }
    }
    None
}

fn parameterized_header(value: &str) -> Option<(&str, Fields)> {
    let (label, mut remaining) = value.split_once(';').unwrap_or((value, ""));
    let mut parameters = Fields::new();
    while !remaining.trim().is_empty() {
        let (key, content) = remaining.split_once('=')?;
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return None;
        }
        let content = content.trim_start();
        let (parameter, next) = if let Some(quoted) = content.strip_prefix('"') {
            let mut parameter = String::new();
            let mut chars = quoted.char_indices();
            let end = loop {
                match chars.next()? {
                    (index, '"') => break index + 1,
                    (_, '\\') => parameter.push(chars.next()?.1),
                    (_, character) => parameter.push(character),
                }
            };
            let tail = quoted[end..].trim_start();
            (
                parameter,
                if tail.is_empty() {
                    ""
                } else {
                    tail.strip_prefix(';')?
                },
            )
        } else {
            let (parameter, next) = content.split_once(';').unwrap_or((content, ""));
            (parameter.trim().to_owned(), next)
        };
        if parameters.insert(key, parameter).is_some() {
            return None;
        }
        remaining = next;
    }
    Some((label.trim(), parameters))
}

fn parse_fields(encoded: &str) -> Result<Fields, Response> {
    let mut fields = Fields::new();
    for (index, pair) in encoded
        .split('&')
        .filter(|part| !part.is_empty())
        .enumerate()
    {
        if index >= 100 {
            return Err(Response::error(400, "The form has too many fields."));
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        fields.insert(decode_form(key)?, decode_form(value)?);
    }
    Ok(fields)
}

fn decode_form(value: &str) -> Result<String, Response> {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let mut source = value.bytes();
    let mut result = Vec::with_capacity(value.len());
    while let Some(byte) = source.next() {
        match byte {
            b'+' => result.push(b' '),
            b'%' => {
                let decoded = source.next().and_then(hex).zip(source.next().and_then(hex));
                let Some((high, low)) = decoded else {
                    return Err(Response::error(400, "Invalid URL encoding."));
                };
                result.push((high << 4) | low);
            }
            byte => result.push(byte),
        }
    }
    String::from_utf8(result).map_err(|_| Response::error(400, "The URL fields must use UTF-8."))
}
