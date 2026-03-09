use std::path::{Path, PathBuf};

use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmtpSecurity {
    StartTls,
    Tls,
    Plain,
}

impl SmtpSecurity {
    fn parse(value: &str) -> Result<Self, MailError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "starttls" => Ok(Self::StartTls),
            "tls" | "ssl" => Ok(Self::Tls),
            "plain" | "none" => Ok(Self::Plain),
            other => Err(MailError::Config(format!(
                "invalid SMTP_SECURITY '{other}', expected one of: starttls, tls, plain"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    from: String,
    from_name: Option<String>,
    security: SmtpSecurity,
    allowed_recipients: Vec<String>,
}

impl SmtpConfig {
    pub fn from_env() -> Result<Self, MailError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    fn from_lookup<F>(get: F) -> Result<Self, MailError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let host = required_env(&get, "SMTP_HOST")?;
        let from = required_env(&get, "SMTP_FROM")?;
        let security = SmtpSecurity::parse(get_trimmed(&get, "SMTP_SECURITY").as_deref().unwrap_or("starttls"))?;
        let port = get_trimmed(&get, "SMTP_PORT")
            .map(|value| {
                value
                    .parse::<u16>()
                    .map_err(|_| MailError::Config(format!("invalid SMTP_PORT '{value}'")))
            })
            .transpose()?
            .unwrap_or_else(|| default_port(security));
        let username = get_trimmed(&get, "SMTP_USERNAME");
        let password = get_trimmed(&get, "SMTP_PASSWORD");
        if username.is_some() ^ password.is_some() {
            return Err(MailError::Config(
                "SMTP_USERNAME and SMTP_PASSWORD must be set together".to_string(),
            ));
        }

        let from_name = get_trimmed(&get, "SMTP_FROM_NAME");
        let allowed_recipients = parse_allowed_recipients(&get)?;

        Ok(Self {
            host,
            port,
            username,
            password,
            from,
            from_name,
            security,
            allowed_recipients,
        })
    }

    fn allows_recipient(&self, recipient: &str) -> bool {
        let normalized = normalize_value(recipient);
        self.allowed_recipients.iter().any(|allowed| allowed == &normalized)
    }
}

#[derive(Debug, Clone)]
pub struct EmailRequest {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub attachment_path: PathBuf,
}

pub async fn send_email(request: EmailRequest) -> Result<(), MailError> {
    let config = SmtpConfig::from_env()?;
    let attachment_bytes = tokio::fs::read(&request.attachment_path)
        .await
        .map_err(|source| MailError::AttachmentRead {
            path: request.attachment_path.clone(),
            source,
        })?;

    let attachment_name = request
        .attachment_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("resume.pdf")
        .to_string();

    let from_address: Address = config
        .from
        .parse()
        .map_err(|_| MailError::Config(format!("invalid SMTP_FROM '{}'", config.from)))?;
    let to_address: Address = request
        .to
        .parse()
        .map_err(|_| MailError::InvalidRecipient(request.to.clone()))?;
    if !config.allows_recipient(&request.to) {
        return Err(MailError::RecipientNotAllowed(request.to));
    }

    let message = Message::builder()
        .from(Mailbox::new(config.from_name.clone(), from_address))
        .to(Mailbox::new(None, to_address))
        .subject(request.subject)
        .multipart(
            MultiPart::mixed()
                .singlepart(SinglePart::plain(request.body))
                .singlepart(Attachment::new(attachment_name).body(
                    attachment_bytes,
                    ContentType::parse("application/pdf").expect("static content-type is valid"),
                )),
        )
        .map_err(|source| MailError::Build(source.to_string()))?;

    let mut transport_builder = match config.security {
        SmtpSecurity::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                .map_err(|source| MailError::Transport(source.to_string()))?
        }
        SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            .map_err(|source| MailError::Transport(source.to_string()))?,
        SmtpSecurity::Plain => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
        }
    }
    .port(config.port);

    if let (Some(username), Some(password)) = (config.username, config.password) {
        transport_builder = transport_builder.credentials(Credentials::new(username, password));
    }

    transport_builder
        .build()
        .send(message)
        .await
        .map_err(|source| MailError::Send(source.to_string()))?;

    Ok(())
}

fn default_port(security: SmtpSecurity) -> u16 {
    match security {
        SmtpSecurity::StartTls => 587,
        SmtpSecurity::Tls => 465,
        SmtpSecurity::Plain => 25,
    }
}

fn get_trimmed<F>(get: &F, key: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    get(key)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn parse_allowed_recipients<F>(get: &F) -> Result<Vec<String>, MailError>
where
    F: Fn(&str) -> Option<String>,
{
    let raw = required_env(get, "SMTP_ALLOWED_RECIPIENTS")?;
    let recipients: Vec<String> = raw
        .split(',')
        .map(normalize_value)
        .filter(|value| !value.is_empty())
        .collect();
    if recipients.is_empty() {
        return Err(MailError::Config(
            "SMTP_ALLOWED_RECIPIENTS must contain at least one email address".to_string(),
        ));
    }
    Ok(recipients)
}

fn required_env<F>(get: &F, key: &str) -> Result<String, MailError>
where
    F: Fn(&str) -> Option<String>,
{
    get_trimmed(get, key)
        .ok_or_else(|| MailError::Config(format!("missing required environment variable {key}")))
}

pub fn default_resume_attachment(workspace: &Path) -> PathBuf {
    workspace.join("resume.pdf")
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("{0}")]
    Config(String),
    #[error("invalid recipient email address: {0}")]
    InvalidRecipient(String),
    #[error("recipient email address is not in SMTP_ALLOWED_RECIPIENTS: {0}")]
    RecipientNotAllowed(String),
    #[error("failed to build email: {0}")]
    Build(String),
    #[error("failed to configure SMTP transport: {0}")]
    Transport(String),
    #[error("failed to read attachment {path}: {source}")]
    AttachmentRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to send email: {0}")]
    Send(String),
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use super::{default_resume_attachment, SmtpConfig};

    #[test]
    fn smtp_config_uses_starttls_defaults() {
        let vars = HashMap::from([
            ("SMTP_HOST", " smtp.example.com ".to_string()),
            ("SMTP_FROM", " bot@example.com ".to_string()),
            ("SMTP_ALLOWED_RECIPIENTS", " user@example.com ".to_string()),
            ("SMTP_USERNAME", " smtp-user ".to_string()),
            ("SMTP_PASSWORD", " smtp-pass ".to_string()),
            ("SMTP_FROM_NAME", " Resume Bot ".to_string()),
        ]);

        let config =
            SmtpConfig::from_lookup(|key| vars.get(key).cloned()).expect("config should parse");

        assert_eq!(config.host, "smtp.example.com");
        assert_eq!(config.port, 587);
        assert_eq!(config.from, "bot@example.com");
        assert_eq!(config.username.as_deref(), Some("smtp-user"));
        assert_eq!(config.password.as_deref(), Some("smtp-pass"));
        assert_eq!(config.from_name.as_deref(), Some("Resume Bot"));
        assert!(config.allows_recipient("USER@example.com"));
    }

    #[test]
    fn smtp_config_requires_matching_credentials() {
        let vars = HashMap::from([
            ("SMTP_HOST", "smtp.example.com".to_string()),
            ("SMTP_FROM", "bot@example.com".to_string()),
            ("SMTP_ALLOWED_RECIPIENTS", "user@example.com".to_string()),
            ("SMTP_USERNAME", "user".to_string()),
        ]);

        let err =
            SmtpConfig::from_lookup(|key| vars.get(key).cloned()).expect_err("config should fail");
        assert!(err
            .to_string()
            .contains("SMTP_USERNAME and SMTP_PASSWORD must be set together"));
    }

    #[test]
    fn smtp_config_uses_security_specific_default_ports() {
        let tls_vars = HashMap::from([
            ("SMTP_HOST", "smtp.example.com".to_string()),
            ("SMTP_FROM", "bot@example.com".to_string()),
            ("SMTP_ALLOWED_RECIPIENTS", "user@example.com".to_string()),
            ("SMTP_SECURITY", "tls".to_string()),
        ]);
        let plain_vars = HashMap::from([
            ("SMTP_HOST", "smtp.example.com".to_string()),
            ("SMTP_FROM", "bot@example.com".to_string()),
            ("SMTP_ALLOWED_RECIPIENTS", "user@example.com".to_string()),
            ("SMTP_SECURITY", "plain".to_string()),
        ]);

        let tls_config =
            SmtpConfig::from_lookup(|key| tls_vars.get(key).cloned()).expect("tls config should parse");
        let plain_config = SmtpConfig::from_lookup(|key| plain_vars.get(key).cloned())
            .expect("plain config should parse");

        assert_eq!(tls_config.port, 465);
        assert_eq!(plain_config.port, 25);
    }

    #[test]
    fn smtp_config_requires_allowed_recipients() {
        let vars = HashMap::from([
            ("SMTP_HOST", "smtp.example.com".to_string()),
            ("SMTP_FROM", "bot@example.com".to_string()),
        ]);

        let err =
            SmtpConfig::from_lookup(|key| vars.get(key).cloned()).expect_err("config should fail");
        assert!(err
            .to_string()
            .contains("missing required environment variable SMTP_ALLOWED_RECIPIENTS"));
    }

    #[test]
    fn default_attachment_points_to_resume_pdf() {
        let path = default_resume_attachment(Path::new("/tmp/workspace"));
        assert_eq!(path, PathBuf::from("/tmp/workspace/resume.pdf"));
    }
}
