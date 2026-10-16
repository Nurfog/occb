//! Envío de email vía SMTP (lettre).
//!
//! - Si `smtp.enabled == false` (o falta host/from) devuelve `Unavailable`
//!   sin intentar conexión: los handlers lo traducen a 503.
//! - Los secretos (password) nunca se loguean; solo host/puerto/destino.

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox,
    transport::smtp::authentication::Credentials,
};

use crate::{config::SmtpConfig, error::AppError};

pub struct EmailMessage {
    pub from: Option<String>,
    pub to: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
}

fn parse_mailbox(addr: &str) -> Result<Mailbox, AppError> {
    addr.trim()
        .parse()
        .map_err(|_| AppError::Validation(format!("email inválido: {addr}")))
}

fn mailboxes(list: Option<&str>) -> Result<Vec<Mailbox>, AppError> {
    let mut out = Vec::new();
    if let Some(list) = list.map(str::trim).filter(|s| !s.is_empty()) {
        for addr in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            out.push(parse_mailbox(addr)?);
        }
    }
    Ok(out)
}

/// Envía un email por SMTP. Respeta `enabled=false` → `Unavailable`.
pub async fn send_via_smtp(smtp: &SmtpConfig, msg: &EmailMessage) -> Result<(), AppError> {
    if !smtp.enabled {
        return Err(AppError::Unavailable);
    }
    if smtp.host.trim().is_empty() || smtp.from.trim().is_empty() {
        tracing::warn!("smtp no configurado (host/from vacíos); envío omitido");
        return Err(AppError::Unavailable);
    }

    let from: Mailbox = match msg.from.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(f) => parse_mailbox(f)?,
        None => parse_mailbox(&smtp.from)?,
    };
    let to = parse_mailbox(&msg.to)?;
    let cc = mailboxes(msg.cc.as_deref())?;
    let bcc = mailboxes(msg.bcc.as_deref())?;

    let mut builder = Message::builder()
        .from(from)
        .to(to)
        .subject(msg.subject.clone());
    for addr in cc {
        builder = builder.cc(addr);
    }
    for addr in bcc {
        builder = builder.bcc(addr);
    }
    let email = match msg.body_html.as_deref().filter(|s| !s.is_empty()) {
        Some(html) => builder
            .multipart(lettre::message::MultiPart::alternative_plain_html(
                msg.body_text.clone(),
                html.to_string(),
            ))
            .map_err(|e| {
                tracing::error!("smtp: no se pudo construir el mensaje");
                let _ = e;
                AppError::Internal
            })?,
        None => builder.body(msg.body_text.clone()).map_err(|_| {
            tracing::error!("smtp: no se pudo construir el mensaje");
            AppError::Internal
        })?,
    };

    let mut transport_builder = if smtp.port == 465 {
        // SMTPS: TLS implícito desde el primer byte.
        AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host).map_err(|e| {
            tracing::error!("smtp: relay inválido para host configurado");
            let _ = e;
            AppError::Internal
        })?
    } else if smtp.port == 587 {
        // Submission: STARTTLS obligatorio (sin downgrade a plaintext).
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host).map_err(|e| {
            tracing::error!("smtp: relay inválido para host configurado");
            let _ = e;
            AppError::Internal
        })?
    } else {
        // Puerto personalizado: relay local de dev (p. ej. Mailpit :1025).
        // Sin TLS: jamás con credenciales (irían en claro).
        if !smtp.user.is_empty() {
            tracing::error!("smtp: credenciales con puerto sin TLS (solo 465/587 en remoto)");
            return Err(AppError::Internal);
        }
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host).port(smtp.port)
    };
    if !smtp.user.is_empty() {
        transport_builder = transport_builder
            .credentials(Credentials::new(smtp.user.clone(), smtp.password.clone()));
    }
    let mailer = transport_builder.build();

    mailer.send(email).await.map_err(|e| {
        // Sin secretos en el log: solo host y error genérico.
        tracing::error!("smtp: fallo de envío via {}: {e:?}", smtp.host);
        AppError::Internal
    })?;
    Ok(())
}
