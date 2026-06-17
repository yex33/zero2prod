use actix_web::HttpResponse;
use actix_web::http::header::ContentType;
use actix_web_flash_messages::IncomingFlashMessages;

pub async fn login_form(flash_messages: IncomingFlashMessages) -> HttpResponse {
    let messages_html = flash_messages
        .iter()
        .map(|m| format!(/* html */ "<p><i>{}</i></p>", m.content()))
        .collect::<Vec<_>>()
        .join("\n");
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(format!(
            /* html */
            r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta http-equiv="content-type" content="text/html" charset="UTF-8">
    <title>Login</title>
</head>
<body>
    {messages_html}
    <form action="/login" method="post">
        <label>Username
            <input type="text" placeholder="Enter Username" name="username">
        </label>
        <label>Password
            <input type="password" placeholder="Enter Password" name="password">
        </label>

        <button type="submit">Login</button>
    </form>
</body>
</html>
        "#,
        ))
}
