use crate::error::{Result, SocketApiError};

pub const FIELD_SEP: char = '\x1e';

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Version,
    GetStrings,
    GetMenuItems { path: String },
    RetrieveFileStatus { path: String },
    RetrieveFolderStatus { path: String },
    Share { path: String },
    MakeAvailableLocally { paths: Vec<String> },
    MakeOnlineOnly { paths: Vec<String> },
    CopyPrivateLink { path: String },
    V2 { name: String, body: String },
}

pub fn parse_command(line: &str) -> Result<Command> {
    if line.is_empty() {
        return Err(SocketApiError::Protocol("empty command line".into()));
    }

    if let Some(rest) = line.strip_prefix("V2/") {
        return Ok(Command::V2 {
            name: rest.to_string(),
            body: String::new(),
        });
    }

    let (cmd, args) = match line.split_once(':') {
        Some((c, a)) => (c, a),
        None => (line, ""),
    };

    match cmd {
        "VERSION" => Ok(Command::Version),
        "GET_STRINGS" => Ok(Command::GetStrings),

        "GET_MENU_ITEMS" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "GET_MENU_ITEMS requires a non-empty path argument".into(),
                ));
            }
            Ok(Command::GetMenuItems { path: args.to_string() })
        }

        "RETRIEVE_FILE_STATUS" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "RETRIEVE_FILE_STATUS requires a path argument".into(),
                ));
            }
            Ok(Command::RetrieveFileStatus { path: args.to_string() })
        }

        "RETRIEVE_FOLDER_STATUS" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "RETRIEVE_FOLDER_STATUS requires a path argument".into(),
                ));
            }
            Ok(Command::RetrieveFolderStatus { path: args.to_string() })
        }

        "SHARE" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "SHARE requires a path argument".into(),
                ));
            }
            Ok(Command::Share { path: args.to_string() })
        }

        "MAKE_AVAILABLE_LOCALLY" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "MAKE_AVAILABLE_LOCALLY requires at least one path argument".into(),
                ));
            }
            let paths = args.split(FIELD_SEP).map(str::to_string).collect();
            Ok(Command::MakeAvailableLocally { paths })
        }

        "MAKE_ONLINE_ONLY" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "MAKE_ONLINE_ONLY requires at least one path argument".into(),
                ));
            }
            let paths = args.split(FIELD_SEP).map(str::to_string).collect();
            Ok(Command::MakeOnlineOnly { paths })
        }

        "COPY_PRIVATE_LINK" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "COPY_PRIVATE_LINK requires a path argument".into(),
                ));
            }
            Ok(Command::CopyPrivateLink { path: args.to_string() })
        }

        other => Err(SocketApiError::Protocol(format!("unknown command: {other:?}"))),
    }
}

pub fn format_response(cmd: &str, parts: &[&str]) -> String {
    if parts.is_empty() {
        format!("{cmd}\n")
    } else {
        format!("{cmd}:{}\n", parts.join(":"))
    }
}
