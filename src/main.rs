use std::error::Error;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::thread;

mod util;

fn main() -> Result<(), Box<dyn Error>> {
    let dir_env = std::env::var("WP360_CODESYS_SOCK_DIR");
    let env_set = dir_env.is_ok();
    let dir = dir_env.unwrap_or("/var/opt/codesyscontrolapi/extfuncs/".to_string());

    let mut buf = vec![0; 1024];
    let mut log = std::collections::VecDeque::<(std::time::SystemTime, String)>::new();

    let dir_path = std::path::Path::new(&dir);
    if !dir_path.is_dir() {
        if env_set {
            return Err(
                format!("Invalid path - \"{dir}\" does not exist or is not a directory").into(),
            );
        }
        let path = std::path::Path::new(&dir);
        util::wait_for(path)?;
    }
    let sock_path = dir_path.join("overdigit-trace-socket.sock");
    match sock_path.try_exists() {
        Ok(true) => {
            match UnixStream::connect(&sock_path) {
                Ok(stream) => {
                    // Socket is in use
                    stream.shutdown(std::net::Shutdown::Both)?;
                    // Gonna let the ::bind call throw an error instead
                }
                Err(_e) => {
                    // Socket is not in use or at the very least not connectable, safe to remove
                    std::fs::remove_file(&sock_path)?;
                }
            }
        }
        Ok(false) => {}
        Err(e) => return Err(Box::new(e)),
    }

    let socket_user =
        std::env::var("WP360_CODESYS_SOCK_USER").unwrap_or("codesyscontrol".to_string());
    let socket_uid = util::get_uid(&socket_user)?.unwrap_or_else(|| {
        panic!(
            "{}",
            format!("Could not find user {socket_user} in /etc/passwd.")
        )
    });

    let listener = UnixDatagram::bind(&sock_path)?;

    std::os::unix::fs::chown(&sock_path, Some(socket_uid), None)?;
    std::fs::set_permissions(&sock_path, Permissions::from_mode(0o775))?;

    listener.set_read_timeout(Some(std::time::Duration::from_millis(100)));

    loop {
        match listener.recv(&mut buf) {
            Ok(bytes) => {
                buf[bytes] = 0; // Terminate the string
                let msg =
                    String::from_utf8((buf[..bytes]).to_vec()).unwrap_or("[ERR]\t\tInvalid string".to_string());
                let item = (std::time::SystemTime::now(), msg);
                println!("{item:?}");
                log.push_back(item);
                // TODO: if there are any active listeners on a (separate) stream socket, format the new item and send it
                log.retain_back(10_000);
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::WouldBlock => {}
                _ => eprintln!("{e:?}"),
            },
        }
        // TODO: if there are any new listeners, send them the entirety of the log, followed by an
        // item with timestamp zero; clients who don't need / want to follow should disconnect
        // after that
    }
    /*
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| ftp::client(stream));
            }
            Err(err) => {
                std::fs::remove_file(&sock_path)?;
                return Err(Box::new(err));
            }
        }
    }*/
    Ok(())
}
