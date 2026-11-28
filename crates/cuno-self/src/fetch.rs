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
    /// Writes what `url` serves to `to`, following redirects.
    fn download(&self, url: &str, to: &Path) -> Result<(), SelfError>;
}

/// A `curl` to run, with the environment cuno was given.
pub struct Curl {
    /// The program, by its location.
    pub program: PathBuf,
    /// The environment it runs with, `PATH` and the proxy variables among
    /// it.
    pub env: Env,
    /// The protocols curl may use, as `--proto` takes them: `=https`, and
    /// more only in tests, which serve files and a local `http://`.
    pub protocols: String,
}

/// How long a connection may take, and how long a transfer may stay below
/// one byte a second, before curl gives up, in seconds. The whole download
/// has no limit, so a large one that is slow but moving is not stopped.
const STALL_SECONDS: &str = "30";

impl Curl {
    /// Windows' own `curl.exe` under `SystemRoot`, else the first `curl.exe`
    /// in an absolute directory of `PATH`.
    pub fn find(env: &Env) -> Result<Curl, SelfError> {
        let system = env
            .set_value("SystemRoot")
            .map(|root| Path::new(root).join(r"System32\curl.exe"));
        let on_path = env.get("PATH").into_iter().flat_map(|path| {
            std::env::split_paths(path)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join("curl.exe"))
                .collect::<Vec<_>>()
        });
        let program = system
            .into_iter()
            .chain(on_path)
            .find(|candidate| candidate.is_file())
            .ok_or(SelfError::NoCurl {
                reason: "no curl.exe in System32 or on PATH".to_owned(),
            })?;
        Ok(Curl {
            program,
            env: env.clone(),
            protocols: "=https".to_owned(),
        })
    }

    /// Runs curl with `args`, ending in the URL, and returns its stdout.
    /// `-f` makes an HTTP error a failure rather than a page saved as the
    /// file; `-sS` keeps the progress meter out and the error in.
    fn run(&self, args: &[&str], url: &str) -> Result<String, SelfError> {
        let output = Command::new(&self.program)
            .args([
                "-fsS",
                "--proto",
                &self.protocols,
                "--proto-redir",
                &self.protocols,
            ])
            .args(["--connect-timeout", STALL_SECONDS])
            .args(["--speed-limit", "1", "--speed-time", STALL_SECONDS])
            .args(args)
            .arg(url)
            .env_clear()
            .envs(self.env.vars())
            .output()
            .map_err(|e| SelfError::NoCurl {
                reason: format!("{}: {e}", self.program.display()),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let lines: Vec<&str> = stderr
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            // curl's own `curl: (N) …` line, which carries the reason.
            let reason = lines
                .iter()
                .rev()
                .find(|l| l.starts_with("curl: ("))
                .or(lines.last())
                .copied()
                .unwrap_or("curl failed with no message")
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
    /// for Windows' 8.8.0) fail with error 43.
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
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

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

    /// This machine's curl, allowed the local server's `http://` and files.
    fn local_curl() -> Curl {
        // The test's own environment, for the curl on this machine; no
        // proxy is asked for the local server.
        let env = Env::from_process().with("NO_PROXY", "127.0.0.1".into());
        Curl {
            protocols: "=https,http,file".to_owned(),
            ..Curl::find(&env).unwrap()
        }
    }

    /// A local HTTP server answering each request by its path: `respond`
    /// returns the whole response. It records `"<METHOD> <path>"` of every
    /// request, and serves until the test ends.
    fn serve(respond: fn(&str) -> String) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut first = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let _ = reader.read_line(&mut first);
                // The rest of the request, up to the blank line.
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or("").to_owned();
                let path = parts.next().unwrap_or("").to_owned();
                log.lock().unwrap().push(format!("{method} {path}"));
                let _ = stream.write_all(respond(&path).as_bytes());
            }
        });
        (base, seen)
    }

    fn response(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// The latest-release lookup asks with `HEAD` and reads the redirect
    /// without following it.
    ///
    /// Mutation: drop `-I`, or follow the redirect; this test fails.
    #[test]
    fn the_redirect_is_asked_with_head_and_not_followed() {
        let (base, seen) = serve(|path| match path {
            "/latest" => response("302 Found", "Location: /tag/v0.2.0\r\n", ""),
            _ => response("200 OK", "", "the tag's page"),
        });
        let to = local_curl().redirect(&format!("{base}/latest")).unwrap();
        assert_eq!(to.as_deref(), Some("/tag/v0.2.0"));
        assert_eq!(*seen.lock().unwrap(), ["HEAD /latest"]);
    }

    /// A download follows the redirect GitHub always answers an asset
    /// with, and saves what it leads to.
    ///
    /// Mutation: drop `-L`, which saves the redirect's empty body; this test
    /// fails.
    #[test]
    fn a_download_follows_redirects() {
        let (base, _) = serve(|path| match path {
            "/asset.zip" => response("302 Found", "Location: /stored/asset.zip\r\n", ""),
            "/stored/asset.zip" => response("200 OK", "", "zip bytes"),
            _ => response("404 Not Found", "", "no"),
        });
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("asset.zip");
        local_curl()
            .download(&format!("{base}/asset.zip"), &to)
            .unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "zip bytes");
    }

    /// An HTTP error is a failure, not a page saved as the file: without
    /// `-f`, curl writes the 404 page and exits 0.
    ///
    /// Mutation: drop `-f`; this test fails.
    #[test]
    fn an_http_error_is_not_a_download() {
        let (base, _) = serve(|_| response("404 Not Found", "", "not found"));
        let dir = tempfile::tempdir().unwrap();
        let url = format!("{base}/missing.zip");
        match local_curl().download(&url, &dir.path().join("got.zip")) {
            Err(SelfError::Download { url: asked, reason }) => {
                assert_eq!(asked, url);
                assert!(reason.starts_with("curl: (22)"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// Only the protocols allowed are used: `http://` is refused by the
    /// curl cuno runs.
    ///
    /// Mutation: drop `--proto`; this test fails.
    #[test]
    fn only_https_is_used() {
        let (base, seen) = serve(|_| response("200 OK", "", "plain"));
        let curl = Curl {
            protocols: "=https".to_owned(),
            ..local_curl()
        };
        let dir = tempfile::tempdir().unwrap();
        let result = curl.download(&format!("{base}/x"), &dir.path().join("x"));
        assert!(
            matches!(result, Err(SelfError::Download { .. })),
            "{result:?}"
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    /// A server that accepts and then says nothing is given up on, rather
    /// than waited on for ever.
    ///
    /// Mutation: drop `--speed-limit` / `--speed-time`; this test fails
    /// (after its own limit).
    #[test]
    fn a_silent_server_is_given_up_on() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        // Accepted and held open, never answered.
        let held = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(90));
            drop(stream);
        });
        let (done, result) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(local_curl().redirect(&url));
        });
        let got = result
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("curl still waiting after 60 s");
        assert!(matches!(got, Err(SelfError::Download { .. })), "{got:?}");
        drop(held);
    }

    fn file_url(path: &Path) -> String {
        format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
    }

    /// A URL that serves nothing is a download error naming that URL and
    /// curl's `curl: (N)` line; a curl that cannot start is told apart from
    /// one that fails.
    ///
    /// Mutation: report another URL, take a line other than curl's, or map
    /// a failed start to a download error; this test fails.
    #[test]
    fn a_failed_download_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let missing = file_url(&dir.path().join("missing.txt"));
        match local_curl().download(&missing, &dir.path().join("x")) {
            Err(SelfError::Download { url, reason }) => {
                assert_eq!(url, missing);
                assert!(reason.starts_with("curl: (37)"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        let absent = Curl {
            program: dir.path().join("no-curl.exe"),
            ..local_curl()
        };
        assert!(matches!(
            absent.download(&missing, &dir.path().join("x")),
            Err(SelfError::NoCurl { .. })
        ));
    }

    /// Windows' own curl comes first, then `PATH`; with neither, there is
    /// no curl.
    ///
    /// Mutation: search `PATH` first, or report a curl that is not there;
    /// this test fails.
    #[test]
    fn curl_is_windows_own_then_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("win");
        let user = dir.path().join("user");
        std::fs::create_dir_all(root.join("System32")).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(user.join("curl.exe"), "").unwrap();
        let env = |pairs: Vec<(&str, std::ffi::OsString)>| {
            Env::from_pairs(
                pairs.into_iter().map(|(k, v)| (k.into(), v)),
                NameCase::Insensitive,
            )
        };
        let path = std::env::join_paths([Path::new("."), &user]).unwrap();
        let found = |e: &Env| Curl::find(e).map(|c| c.program);
        let both = env(vec![
            ("SystemRoot", root.clone().into()),
            ("PATH", path.clone()),
        ]);
        assert_eq!(found(&both), Ok(user.join("curl.exe")));
        std::fs::write(root.join(r"System32\curl.exe"), "").unwrap();
        assert_eq!(found(&both), Ok(root.join(r"System32\curl.exe")));
        let none = env(vec![
            ("SystemRoot", dir.path().join("nowhere").into()),
            ("PATH", ".".into()),
        ]);
        assert!(matches!(found(&none), Err(SelfError::NoCurl { .. })));
    }
}
