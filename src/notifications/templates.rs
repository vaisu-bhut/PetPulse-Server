use serde::Serialize;

pub struct NotificationTemplates;

impl NotificationTemplates {
    /// Generates a rich HTML email template for critical alerts
    pub fn critical_alert_email(
        pet_name: &str,
        severity: &str,
        description: &str,
        started_at: &str,
        critical_indicators: &[String],
        recommended_actions: &[String],
        video_link: &str,
    ) -> String {
        let indicators_html = critical_indicators
            .iter()
            .map(|i| format!("<li>{}</li>", i))
            .collect::<Vec<_>>()
            .join("");

        let actions_html = recommended_actions
            .iter()
            .map(|a| format!("<li>{}</li>", a))
            .collect::<Vec<_>>()
            .join("");

        format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: 'Helvetica Neue', Helvetica, Arial, sans-serif; line-height: 1.6; color: #333; }}
        .container {{ max-width: 600px; margin: 0 auto; padding: 20px; border: 1px solid #ddd; border-radius: 8px; }}
        .header {{ background-color: #dfe6e9; padding: 15px; border-radius: 8px 8px 0 0; text-align: center; }}
        .header h1 {{ margin: 0; color: #2d3436; }}
        .alert-badge {{ background-color: #d63031; color: white; padding: 5px 10px; border-radius: 4px; font-weight: bold; display: inline-block; margin-top: 10px; }}
        .content {{ padding: 20px; }}
        .section {{ margin-bottom: 20px; }}
        .section h3 {{ border-bottom: 2px solid #eee; padding-bottom: 5px; color: #636e72; }}
        .button {{ display: inline-block; background-color: #0984e3; color: white; padding: 10px 20px; text-decoration: none; border-radius: 5px; font-weight: bold; }}
        .footer {{ margin-top: 30px; font-size: 12px; color: #b2bec3; text-align: center; }}
        ul {{ padding-left: 20px; }}
        li {{ margin-bottom: 5px; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚨 PetPulse Critical Alert</h1>
            <div class="alert-badge">SEVERITY: {severity}</div>
        </div>
        <div class="content">
            <p><strong>Immediate Attention Required for {pet_name}</strong></p>
            <p>{description}</p>
            <p><strong>Time:</strong> {started_at}</p>

            <div class="section">
                <h3>⚠️ Critical Indicators Observed</h3>
                <ul>
                    {indicators_html}
                </ul>
            </div>

            <div class="section">
                <h3>📋 Recommended Actions</h3>
                <ul>
                    {actions_html}
                </ul>
            </div>

            <div class="section" style="text-align: center; margin-top: 30px;">
                <a href="{video_link}" class="button">📺 View Video Clip</a>
            </div>
            
            <p style="text-align: center; margin-top: 20px;">
                <small>This link expires in 24 hours.</small>
            </p>
        </div>
        <div class="footer">
            <p>Sent by PetPulse Autonomous Monitoring System</p>
        </div>
    </div>
</body>
</html>
"#,
            severity = severity.to_uppercase(),
            pet_name = pet_name,
            description = description,
            started_at = started_at,
            indicators_html = indicators_html,
            actions_html = actions_html,
            video_link = video_link
        )
    }

    /// Generates a concise SMS message
    pub fn critical_alert_sms(
        pet_name: &str,
        severity: &str,
        description: &str,
        video_link: &str,
    ) -> String {
        // Truncate description if too long
        let short_desc = if description.len() > 50 {
            format!("{}...", &description[..47])
        } else {
            description.to_string()
        };

        format!(
            "🚨 PetPulse ALERT: {} - {}\nSeverity: {}\nView: {}",
            pet_name,
            short_desc,
            severity.to_uppercase(),
            video_link
        )
    }
}
