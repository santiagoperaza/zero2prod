use crate::domain::SubscriberEmail;
use reqwest::Client;
use secrecy::{ExposeSecret, Secret};

#[derive(Debug)]
pub struct EmailClient {
    http_client: Client,
    base_url: String,
    sender: SubscriberEmail,
    authorization_token: Secret<String>,
}

impl EmailClient {
    pub fn new(
        base_url: String,
        sender: SubscriberEmail,
        authorization_token: Secret<String>,
        timeout: std::time::Duration,
    ) -> Self {
        let http_client = Client::builder().timeout(timeout).build().unwrap();
        Self {
            http_client,
            base_url,
            sender,
            authorization_token,
        }
    }

    #[tracing::instrument(
        name = "Sending email",
        // skip(new_subscriber, transaction)
    )]
    pub async fn send_email(
        &self,
        recipient: SubscriberEmail,
        subject: &str,
        html_content: &str,
        text_content: &str,
    ) -> Result<(), reqwest::Error> {
        let url = format!("{}/mail/send", self.base_url);
        let request_body = SendEmailRequest {
            personalizations: vec![Personalization {
                to: vec![Recipient {
                    email: recipient.as_ref(),
                }],
            }],
            from: Email {
                email: self.sender.as_ref(),
            },
            subject,
            content: vec![
                Content {
                    r#type: "text/plain",
                    value: text_content,
                },
                Content {
                    r#type: "text/html",
                    value: html_content,
                },
            ],
        };

        print!("request_body: {:?}", request_body);
        print!("secret: {:?}", self.authorization_token.expose_secret());
        print!("url: {:?}", &url);

        self.http_client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.authorization_token.expose_secret()),
            )
            .json(&request_body)
            .send()
            .await?
            .error_for_status()?;
        print!("SUCCESS: {:?}", request_body);
        Ok(())
    }
}

#[derive(serde::Serialize, Debug)]
struct SendEmailRequest<'a> {
    personalizations: Vec<Personalization<'a>>,
    from: Email<'a>,
    subject: &'a str,
    content: Vec<Content<'a>>,
}

#[derive(serde::Serialize, Debug)]
struct Personalization<'a> {
    to: Vec<Recipient<'a>>,
}

#[derive(serde::Serialize, Debug)]
struct Recipient<'a> {
    email: &'a str,
}

#[derive(serde::Serialize, Debug)]
struct Email<'a> {
    email: &'a str,
}

#[derive(serde::Serialize, Debug)]
struct Content<'a> {
    #[serde(rename = "type")]
    r#type: &'a str,
    value: &'a str,
}

#[cfg(test)]
mod tests {
    use crate::domain::SubscriberEmail;
    use crate::email_client::EmailClient;
    use claims::{assert_err, assert_ok};
    use fake::faker::internet::en::SafeEmail;
    use fake::faker::lorem::en::{Paragraph, Sentence};
    use fake::{Fake, Faker};
    use secrecy::Secret;
    use wiremock::matchers::{any, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    struct SendEmailBodyMatcher;

    impl wiremock::Match for SendEmailBodyMatcher {
        fn matches(&self, request: &Request) -> bool {
            let result: Result<serde_json::Value, _> = serde_json::from_slice(&request.body);
            if let Ok(body) = result {
                body.get("from").is_some()
                    && body.get("personalizations").is_some()
                    && body.get("subject").is_some()
                    && body.get("content").is_some()
            } else {
                false
            }
        }
    }

    /// Generate a random email subject
    fn subject() -> String {
        Sentence(1..2).fake()
    }

    /// Generate a random email content
    fn content() -> String {
        Paragraph(1..10).fake()
    }

    /// Generate a random subscriber email
    fn email() -> SubscriberEmail {
        SubscriberEmail::parse(SafeEmail().fake()).unwrap()
    }

    /// Get a test instance of `EmailClient`.
    fn email_client(base_url: String) -> EmailClient {
        EmailClient::new(
            base_url,
            email(),
            Secret::new(Faker.fake()),
            std::time::Duration::from_millis(200),
        )
    }

    #[tokio::test]
    async fn send_email_sends_the_expected_request() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(header_exists("Authorization"))
            .and(header("Content-Type", "application/json"))
            .and(path("/mail/send"))
            .and(method("POST"))
            .and(SendEmailBodyMatcher)
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let _ = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
    }

    #[tokio::test]
    async fn send_email_succeeds_if_the_server_returns_200() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let outcome = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
        assert_ok!(outcome);
    }

    #[tokio::test]
    async fn send_email_fails_if_the_server_returns_500() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(any())
            // Not a 200 anymore!
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let outcome = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
        assert_err!(outcome);
    }

    #[tokio::test]
    async fn send_email_times_out_if_the_server_takes_too_long() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        let response = ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(180));
        Mock::given(any())
            .respond_with(response)
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let outcome = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
        assert_err!(outcome);
    }
}
