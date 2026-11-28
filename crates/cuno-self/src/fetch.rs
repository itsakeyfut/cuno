//! Downloading with `curl` (`docs/en/specs/distribution.md`, "Downloads"):
//! Windows' own verifies TLS against the Windows certificate store and
//! follows the proxy variables, so nothing has to be configured.

use std::path::{Path, PathBuf};
use std::process::Command;

use cuno_core::Env;

use crate::SelfError;

/// What the update asks of the network. The binary always uses [`Curl`];
/// tests answer from files.
pub trait Fetch {
    /// Where `url` redirects to, without following it: `None` when it does
    /// not redirect.
    fn redirect(&self, url: &str) -> Result<Option<String>, SelfError>;
    /// Writes what `url` serves to `to`.
    fn download(&self, url: &str, to: &Path) -> Result<(), SelfError>;
}

/// `curl`, found on `PATH`, run with the environment cuno was given.
pub struct Curl {
    /// The program to run: `curl`, looked up on `PATH`.
    pub program: PathBuf,
    /// The environment it runs with, `PATH` and the proxy variables among
    /// it.
    pub env: Env,
}

impl Curl {
    /// `curl` as `PATH` in `env` finds it.
    pub fn new(env: &Env) -> Curl {
        Curl {
            program: PathBuf::from("curl"),
            env: env.clone(),
        }
    }

    /// Runs curl with `args`, ending in the URL, and returns its stdout.
    /// `-f` makes an HTTP error a failure rather than a page saved as the
    /// file; `-sS` keeps the progress meter out and the error in.
    fn run(&self, args: &[&str], url: &str) -> Result<String, SelfError> {
        let output = Command::new(&self.program)
            .args(["-fsS"])
            .args(args)
            .arg(url)
            .env_clear()
            .envs(self.env.vars())
            .output()
            .map_err(|e| SelfError::NoCurl {
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("curl failed with no message")
                .trim()
                .to_owned();
            return Err(SelfError::Download {
                url: url.to_owned(),
                reason,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Fetch for Curl {
    /// The `Location` of a `HEAD` request. It is read from the headers
    /// rather than with `-w '%{url_effective}'`, which some curl builds (Git
    /// for Windows' 8.8.0, found first on some `PATH`s) fail with error 43.
    fn redirect(&self, url: &str) -> Result<Option<String>, SelfError> {
        let headers = self.run(&["-I"], url)?;
        Ok(location(&headers))
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), SelfError> {
        let to = to.to_string_lossy();
        self.run(&["-L", "-o", &to], url).map(|_| ())
    }
}

/// The last `Location` header in `headers`, as curl prints them.
fn location(headers: &str) -> Option<String> {
    headers
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("location")
                .then(|| value.trim().to_owned())
        })
        .rfind(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::NameCase;

    /// The `Location` header, in any case, the last of several when curl
    /// printed more than one response.
    ///
    /// Mutation: match the name case-sensitively, or take the first; this
    /// test fails.
    #[test]
    fn the_redirect_is_the_location_header() {
        let headers = "HTTP/1.1 302 Found\r\nlocation: https://a/tag/v0.1.0\r\nx: y\r\n\r\nHTTP/1.1 302 Found\r\nLocation:  https://a/tag/v0.2.0 \r\n\r\n";
        assert_eq!(location(headers).as_deref(), Some("https://a/tag/v0.2.0"));
        assert_eq!(
            location("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n"),
            None
        );
    }

    fn file_url(path: &Path) -> String {
        format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
    }

    /// Serves one HTTP request on a local port with `404 Not Found` and a
    /// body, and returns the URL to ask.
    fn not_found_once() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/missing.zip", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let body = "not found";
            let _ = write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        url
    }

    /// An HTTP error is a failure, not a page saved as the file: without
    /// `-f`, curl writes the 404 page and exits 0.
    ///
    /// Mutation: drop `-f`; this test fails.
    #[test]
    fn an_http_error_is_not_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let url = not_found_once();
        // `NO_PROXY` so a proxy set on this machine is not asked for
        // localhost.
        let env = Env::from_process().with("NO_PROXY", "127.0.0.1".into());
        let to = dir.path().join("got.zip");
        match Curl::new(&env).download(&url, &to) {
            Err(SelfError::Download { url: asked, reason }) => {
                assert_eq!(asked, url);
                assert!(reason.contains("404"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A real curl writes the file a URL serves, and a URL that serves
    /// nothing is a download error naming the URL and curl's message; a
    /// curl that cannot start is told apart from one that fails.
    ///
    /// Mutation: report another URL or drop curl's message, or map a
    /// failed start to a download error; this test fails.
    #[test]
    fn curl_downloads_or_says_why_not() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("served.txt");
        std::fs::write(&from, "payload").unwrap();
        // The test's own environment, for the curl on this machine's PATH.
        let curl = Curl::new(&Env::from_process());
        let to = dir.path().join("got.txt");
        curl.download(&file_url(&from), &to).unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "payload");

        let missing = file_url(&dir.path().join("missing.txt"));
        match curl.download(&missing, &dir.path().join("x")) {
            Err(SelfError::Download { url, reason }) => {
                assert_eq!(url, missing);
                assert!(reason.starts_with("curl: ("), "{reason}");
            }
            other => panic!("{other:?}"),
        }

        let absent = Curl {
            program: dir.path().join("no-curl.exe"),
            env: Env::from_pairs([], NameCase::Insensitive),
        };
        assert!(matches!(
            absent.download(&file_url(&from), &to),
            Err(SelfError::NoCurl { .. })
        ));
    }
}
