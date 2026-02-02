use sendgrid::SGClient;
use sendgrid::{Destination, Mail};
use std::env;
use tracing::{error, info, warn};
use super::NotificationTemplates;
use super::PubSubClient; // Import PubSubClient
use super::pubsub_client::AlertEmailPayload;

#[derive(Clone)]
pub struct TwilioNotifier {
    sendgrid_client: Option<SGClient>,
    twilio_client: Option<twilio::Client>,
    pub_sub_client: Option<PubSubClient>,
    sms_from: String,
    email_from: String,
}

impl TwilioNotifier {
    pub async fn new() -> Self {
        let sendgrid_api_key = env::var("TWILIO_SENDGRID_API_KEY").ok();
        let twilio_account_sid = env::var("TWILIO_ACCOUNT_SID").ok();
        let twilio_auth_token = env::var("TWILIO_AUTH_TOKEN").ok();
        let sms_from = env::var("TWILIO_SMS_FROM_NUMBER").unwrap_or_default();
        let email_from = env::var("NOTIFICATION_EMAIL_FROM").unwrap_or_else(|_| "alerts@petpulse.com".to_string());

        let sendgrid_client = sendgrid_api_key.map(|key| SGClient::new(key));
        
        let twilio_client = if let (Some(sid), Some(token)) = (twilio_account_sid, twilio_auth_token) {
            Some(twilio::Client::new(&sid, &token))
        } else {
            None
        };

        let pub_sub_client = match PubSubClient::new().await {
            Ok(client) => Some(client),
            Err(e) => {
                warn!("⚠️ Failed to initialize PubSubClient: {}. Email alerts via Cloud Functions will not work.", e);
                None
            }
        };

        if sendgrid_client.is_none() {
            warn!("⚠️ SendGrid API key not found. Email notifications will be mocked (unless PubSub is used).");
        }
        if twilio_client.is_none() {
            warn!("⚠️ Twilio credentials not found. SMS notifications will be mocked.");
        }

        Self {
            sendgrid_client,
            twilio_client,
            pub_sub_client,
            sms_from,
            email_from,
        }
    }

    pub async fn send_email(
        &self,
        to_email: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), String> {
        if let Some(client) = &self.sendgrid_client {
            // Must own data to move into closure for static lifetime
            let to_email = to_email.to_string();
            let subject = subject.to_string();
            let body = body.to_string();
            let email_from = self.email_from.clone();
            let client = client.clone();
            let to_email_log = to_email.clone();

             match tokio::task::spawn_blocking(move || {
                let mail_info = Mail::new()
                    .add_to(Destination {
                        address: &to_email,
                        name: "Pet Owner",
                    })
                    .add_from(&email_from)
                    .add_subject(&subject)
                    .add_html(&body);
                    
                client.send(mail_info)
            }).await {
                 Ok(result) => match result {
                    Ok(_) => {
                        info!("✅ Email sent successfully to {}", to_email_log);
                        crate::metrics::increment_notifications_sent("email");
                        Ok(())
                    }
                    Err(e) => {
                        error!("❌ Failed to send email: {}", e);
                        crate::metrics::increment_notifications_failed("email");
                        Err(format!("SendGrid Error: {}", e))
                    }
                 },
                 Err(e) => Err(format!("Task Join Error: {}", e))
            }
        } else {
            // Mock mode
            info!("(Mock) 📧 Would send email to: {}", to_email);
            info!("(Mock) Subject: {}", subject);
            info!("(Mock) Body length: {} chars", body.len());
            crate::metrics::increment_notifications_sent("email");
            Ok(())
        }
    }

    pub async fn send_sms(
        &self,
        to_number: &str,
        body: &str,
    ) -> Result<(), String> {
        if let Some(client) = &self.twilio_client {
            if self.sms_from.is_empty() {
                return Err("TWILIO_SMS_FROM_NUMBER not set".to_string());
            }

            // Using the blocking client in async context (reqwest/twilio crate limitation or design)
            // Ideally we'd wrap this or use an async-compatible client method if available
            // For now, simple approach:
            
            match client.send_message(
                twilio::OutboundMessage::new(&self.sms_from, to_number, body)
            ).await {
                Ok(_) => {
                    info!("✅ SMS sent successfully to {}", to_number);
                    crate::metrics::increment_notifications_sent("sms");
                    Ok(())
                }
                Err(e) => {
                    error!("❌ Failed to send SMS: {}", e);
                    crate::metrics::increment_notifications_failed("sms");
                    Err(format!("Twilio Error: {}", e))
                }
            }
        } else {
            // Mock mode
            info!("(Mock) 📱 Would send SMS to: {}", to_number);
            info!("(Mock) Body: {}", body);
            crate::metrics::increment_notifications_sent("sms");
            Ok(())
        }
    }

    pub async fn notify_critical_alert(
        &self,
        owner_email: &str,
        owner_phone: &str,
        pet_name: &str,
        severity: &str,
        description: &str,
        critical_indicators: &[String],
        recommended_actions: &[String],
        video_link: &str,
    ) {
        
        // 1. Send Email via Pub/Sub (Cloud Function)
        if let Some(pub_sub) = &self.pub_sub_client {
             // We need an ID for the alert to generate a link, but we don't have it passed here easily unless we change the signature.
             // The Cloud Function expects 'id' for the link: /alerts/{id}
             // For now, we'll use a placeholder or generate a random one if not provided, 
             // BUT ideally the caller should provide the Alert ID.
             // Assuming description contains enough info or we pass "unknown".
             // Actually, verify_escalation.sh doesn't seem to pass ID to this flow maybe? 
             // Let's check call site.
             
             // Update: We'll construct a simple list string for the message
             let message = format!("{}\n\nIndicators: {:?}\n\nActions: {:?}", description, critical_indicators, recommended_actions);

             let payload = AlertEmailPayload {
                 email: owner_email.to_string(),
                 pet_name: pet_name.to_string(),
                 message,
                 severity: severity.to_string(),
                 id: "latest".to_string(), // Metadata unavailable in this signature, TODO: Update signature
                 title: Some(format!("Critical Alert for {}", pet_name)),
             };
             
             pub_sub.publish_email_alert(payload).await;
        } else {
             // Fallback to legacy direct email if PubSub not available
             let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
             let email_body = NotificationTemplates::critical_alert_email(
                 pet_name,
                 severity,
                 description,
                 &timestamp,
                 critical_indicators,
                 recommended_actions,
                 video_link,
             );
             
             let subject = format!("🚨 CRITICAL ALERT: {} needs attention!", pet_name);
             let email_notifier = self.clone();
             let email_target = owner_email.to_string();
             tokio::spawn(async move {
                 let _ = email_notifier.send_email(&email_target, &subject, &email_body).await;
             });
        }

        // 2. Send SMS
        let sms_body = NotificationTemplates::critical_alert_sms(
            pet_name,
            severity,
            description,
            video_link,
        );

        // Spawn SMS task
        let sms_notifier = self.clone();
        let sms_target = owner_phone.to_string();
        tokio::spawn(async move {
            let _ = sms_notifier.send_sms(&sms_target, &sms_body).await;
        });
    }
}
