use crate::storage::admin::AdminPasteItem;
use askama::Template;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub base_url: String,
}

#[derive(Template)]
#[template(path = "view.html")]
pub struct ViewTemplate {
    pub id: String,
    pub content: String,
    pub content_type: String,
    pub size: String,
    pub expires_at: String,
    pub is_text: bool,
}

#[derive(Template)]
#[template(path = "admin/login.html")]
pub struct AdminLoginTemplate {
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "admin/setup.html")]
pub struct AdminSetupTemplate {
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "admin/list.html")]
pub struct AdminListTemplate {
    pub pastes: Vec<AdminPasteItem>,
}
