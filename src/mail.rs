use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use base64ct::{Base64, Encoding};

const SMTP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SMTP_REPLY: usize = 512;
const MAX_MESSAGE_BODY: usize = 16 * 1024;
const LOCAL_FROM: &str = "threadporch@localhost";
const RESET_QUEUE_CAPACITY: usize = 32;

pub struct AccountMail {
    delivery: MailDelivery,
    reset_queue: Option<SyncSender<(String, String)>>,
}

impl AccountMail {
    pub fn from_env() -> io::Result<Self> {
        let delivery = MailDelivery::from_env()?;
        let reset_queue = if delivery.configured() {
            let (sender, receiver) = mpsc::sync_channel::<(String, String)>(RESET_QUEUE_CAPACITY);
            let reset_delivery = delivery.clone();
            thread::Builder::new()
                .name("threadporch-reset-mail".into())
                .spawn(move || {
                    while let Ok((address, body)) = receiver.recv() {
                        if reset_delivery
                            .send(&address, "Reset your ThreadPorch password", &body)
                            .is_err()
                        {
                            eprintln!("ThreadPorch could not deliver a password-reset message.");
                        }
                    }
                })?;
            Some(sender)
        } else {
            None
        };
        Ok(Self {
            delivery,
            reset_queue,
        })
    }

    pub fn configured(&self) -> bool {
        self.delivery.configured()
    }

    pub fn send_verification(&self, address: &str, body: &str) -> io::Result<()> {
        self.delivery
            .send(address, "Verify your ThreadPorch email", body)
    }

    pub fn queue_reset(&self, address: String, body: String) {
        if let Some(sender) = &self.reset_queue {
            if sender.try_send((address, body)).is_err() {
                eprintln!("ThreadPorch could not queue a password-reset message.");
            }
        }
    }
}

#[derive(Clone)]
pub enum MailDelivery {
    Disabled,
    Smtp { address: SocketAddr, from: String },
    Outbox { directory: PathBuf, from: String },
}

impl MailDelivery {
    pub fn from_env() -> io::Result<Self> {
        let smtp = env_text("COMMONLINE_SMTP_ADDR")?;
        let outbox = env::var_os("COMMONLINE_MAIL_OUTBOX").filter(|path| !path.is_empty());
        if smtp.is_some() && outbox.is_some() {
            return Err(invalid(
                "Choose either COMMONLINE_SMTP_ADDR or COMMONLINE_MAIL_OUTBOX.",
            ));
        }
        if smtp.is_none() && outbox.is_none() {
            return Ok(Self::Disabled);
        }
        let from = match env_text("COMMONLINE_MAIL_FROM")? {
            Some(from) if valid_address(&from) => from,
            Some(_) => {
                return Err(invalid(
                    "COMMONLINE_MAIL_FROM must be a plain email address.",
                ))
            }
            None if smtp.is_some() => {
                return Err(invalid("Set COMMONLINE_MAIL_FROM for SMTP delivery."))
            }
            None => LOCAL_FROM.to_owned(),
        };
        if let Some(address) = smtp {
            let address = address
                .parse::<SocketAddr>()
                .ok()
                .filter(|address| address.ip().is_loopback() && address.port() != 0)
                .ok_or_else(|| {
                    invalid("COMMONLINE_SMTP_ADDR must be a literal loopback address and port.")
                })?;
            return Ok(Self::Smtp { address, from });
        }
        let directory = PathBuf::from(
            outbox.ok_or_else(|| invalid("Set COMMONLINE_MAIL_OUTBOX for local delivery."))?,
        );
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        Ok(Self::Outbox { directory, from })
    }

    pub fn configured(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn send(&self, to: &str, subject: &str, body: &str) -> io::Result<()> {
        let from = match self {
            Self::Disabled => return Err(io::Error::other("Email delivery is unavailable.")),
            Self::Smtp { from, .. } | Self::Outbox { from, .. } => from,
        };
        if !valid_address(from) || !valid_address(to) {
            return Err(invalid(
                "Email requires a plain sender and recipient address.",
            ));
        }
        if subject.is_empty()
            || subject.len() > 160
            || !subject
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
            || body.len() > MAX_MESSAGE_BODY
        {
            return Err(invalid("Invalid email subject or body."));
        }
        let mut bytes = [0_u8; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|_| io::Error::other("Could not generate a mail message identifier."))?;
        let mut id = String::with_capacity(32);
        for byte in bytes {
            write!(id, "{byte:02x}").unwrap();
        }
        let message = message(from, to, subject, body, &id)?;
        match self {
            Self::Smtp { address, .. } => deliver(*address, from, to, &message),
            Self::Outbox { directory, .. } => store_outbox(directory, &id, &message),
            Self::Disabled => unreachable!(),
        }
    }
}

fn env_text(name: &str) -> io::Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_owned())),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(invalid("Invalid mail configuration value.")),
    }
}

pub(super) fn valid_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    address.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && local.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&byte)
                })
        })
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn message(from: &str, to: &str, subject: &str, body: &str, id: &str) -> io::Result<String> {
    let date = mail_date()?;
    let domain = from
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .ok_or_else(|| invalid("Invalid mail sender address."))?;
    let plain = body.replace("\r\n", "\n").replace('\r', "\n");
    let seven_bit = plain
        .bytes()
        .all(|byte| matches!(byte, b'\t' | b'\n' | b' ') || byte.is_ascii_graphic())
        && plain.lines().all(|line| line.len() <= 998);
    let encoding = if seven_bit { "7bit" } else { "base64" };
    let mut result = format!(
        "Date: {date}\r\nFrom: {from}\r\nTo: {to}\r\nSubject: {subject}\r\n\
         Message-ID: <{id}@{domain}>\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n"
    );
    if seven_bit {
        for line in plain.split_terminator('\n') {
            write!(result, "{line}\r\n").unwrap();
        }
    } else {
        let canonical = plain.replace('\n', "\r\n");
        let encoded = Base64::encode_string(canonical.as_bytes());
        for line in encoded.as_bytes().chunks(76) {
            result.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
            result.push_str("\r\n");
        }
    }
    Ok(result)
}

fn mail_date() -> io::Result<String> {
    let now = crate::schedule_time::now();
    let utc = crate::schedule_time::to_local(now, "UTC");
    let (date, clock) = utc
        .split_once('T')
        .ok_or_else(|| io::Error::other("Could not create a mail date."))?;
    let parts: Vec<_> = date.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return Err(io::Error::other("Could not create a mail date."));
    };
    let names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = month
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| names.get(index))
        .ok_or_else(|| io::Error::other("Could not create a mail date."))?;
    Ok(format!(
        "{day} {month} {year} {clock}:{:02} +0000",
        now % 60
    ))
}

fn store_outbox(directory: &Path, id: &str, message: &str) -> io::Result<()> {
    let temporary = directory.join(format!(".{id}.tmp"));
    let destination = directory.join(format!("{id}.eml"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = file
        .write_all(message.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temporary, destination));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn deliver(address: SocketAddr, from: &str, to: &str, message: &str) -> io::Result<()> {
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(invalid("SMTP delivery requires a nonzero loopback socket."));
    }
    let deadline = Instant::now() + SMTP_TIMEOUT;
    let stream = TcpStream::connect_timeout(&address, SMTP_TIMEOUT)?;
    let mut stream = BufReader::new(stream);
    expect_reply(read_reply(&mut stream, deadline)?, &[220])?;
    let hello = command(&mut stream, "EHLO localhost\r\n", deadline)?;
    if matches!(hello, 500 | 502 | 504) {
        expect_reply(
            command(&mut stream, "HELO localhost\r\n", deadline)?,
            &[250],
        )?;
    } else {
        expect_reply(hello, &[250])?;
    }
    expect_reply(
        command(&mut stream, &format!("MAIL FROM:<{from}>\r\n"), deadline)?,
        &[250],
    )?;
    expect_reply(
        command(&mut stream, &format!("RCPT TO:<{to}>\r\n"), deadline)?,
        &[250, 251, 252],
    )?;
    expect_reply(command(&mut stream, "DATA\r\n", deadline)?, &[354])?;
    let data = dot_stuff(message);
    expect_reply(command(&mut stream, &data, deadline)?, &[250])?;
    let quit_deadline = deadline.min(Instant::now() + Duration::from_millis(250));
    let _ = command(&mut stream, "QUIT\r\n", quit_deadline);
    Ok(())
}

fn dot_stuff(message: &str) -> String {
    let mut data = String::with_capacity(message.len() + 3);
    for line in message.split_inclusive('\n') {
        if line.starts_with('.') {
            data.push('.');
        }
        data.push_str(line);
    }
    data.push_str(".\r\n");
    data
}

fn command(stream: &mut BufReader<TcpStream>, command: &str, deadline: Instant) -> io::Result<u16> {
    write_smtp(stream, command, deadline)?;
    read_reply(stream, deadline)
}

fn write_smtp(
    stream: &mut BufReader<TcpStream>,
    message: &str,
    deadline: Instant,
) -> io::Result<()> {
    stream
        .get_ref()
        .set_write_timeout(Some(remaining(deadline)?))?;
    stream.get_mut().write_all(message.as_bytes())
}

fn read_reply(stream: &mut BufReader<TcpStream>, deadline: Instant) -> io::Result<u16> {
    let mut previous = None;
    for _ in 0..64 {
        let line = read_reply_line(stream, deadline)?;
        let code: u16 = std::str::from_utf8(line.get(..3).unwrap_or_default())
            .ok()
            .and_then(|code| code.parse().ok())
            .filter(|code| (200..=599).contains(code))
            .ok_or_else(invalid_reply)?;
        if previous.is_some_and(|previous| previous != code) {
            return Err(invalid_reply());
        }
        previous = Some(code);
        match line.get(3..) {
            Some([b'-', ..]) => {}
            Some([b' ', ..]) | Some([]) => return Ok(code),
            _ => return Err(invalid_reply()),
        }
    }
    Err(invalid_reply())
}

fn read_reply_line(stream: &mut BufReader<TcpStream>, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        stream
            .get_ref()
            .set_read_timeout(Some(remaining(deadline)?))?;
        let buffer = stream.fill_buf()?;
        if buffer.is_empty() {
            return Err(invalid_reply());
        }
        let end = buffer.iter().position(|&byte| byte == b'\n');
        let count = end.map_or(buffer.len(), |index| index + 1);
        if line.len() + count > MAX_SMTP_REPLY {
            return Err(invalid_reply());
        }
        line.extend_from_slice(&buffer[..count]);
        stream.consume(count);
        if end.is_some() {
            if !line.ends_with(b"\r\n") {
                return Err(invalid_reply());
            }
            line.truncate(line.len() - 2);
            return Ok(line);
        }
    }
}

fn expect_reply(code: u16, success: &[u16]) -> io::Result<()> {
    if success.contains(&code) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Local SMTP relay rejected delivery (status {code})."
        )))
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|time| !time.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Local SMTP relay timed out."))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_reply() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid reply from the local SMTP relay.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_address_and_header_validation_precede_network_activity() {
        assert!(valid_address("reader.name+test@example.test"));
        assert!(!valid_address(
            "reader@example.test\r\nBcc: other@example.test"
        ));
        assert!(!valid_address("a..b@example.test"));
        assert!(!valid_address("reader@example.test>"));
        let delivery = MailDelivery::Smtp {
            address: "203.0.113.1:25".parse().unwrap(),
            from: "threadporch@example.test".into(),
        };
        for (address, subject) in [
            (
                "reader@example.test\r\nBcc: other@example.test",
                "Verify email",
            ),
            ("reader@example.test", "Verify email\r\nX-Injected: test"),
            ("reader@example.test", "Verify email"),
        ] {
            assert_eq!(
                delivery
                    .send(address, subject, "Fixture message")
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn mime_and_smtp_stuffing_preserve_literal_message_body() {
        let from = "threadporch@example.test";
        let to = "reader@example.test";
        let sample = message(from, to, "Verify email", ".first\n.\nLast line", "sample").unwrap();
        assert!(sample.contains("To: reader@example.test\r\n"));
        assert!(sample.contains("Content-Transfer-Encoding: 7bit\r\n"));
        assert!(dot_stuff(&sample).ends_with("\r\n..first\r\n..\r\nLast line\r\n.\r\n"));
        let unicode = message(from, to, "Verify email", "\u{00E9}", "sample").unwrap();
        assert!(unicode.contains("Content-Transfer-Encoding: base64\r\n\r\nw6k=\r\n"));
    }
}
