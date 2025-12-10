use std::collections::HashMap;

use html2md::dummy::DummyHandler;
use html2md::parse_html_custom;
use html2md::Handle;
use html2md::NodeData;
use html2md::StructuredPrinter;
use html2md::TagHandler;

use html2md::common::get_tag_attr;
use html2md::walk;
use html2md::TagHandlerFactory;

use url::Url;

#[derive(Default)]
pub struct IgnoreHandler;

impl TagHandler for IgnoreHandler {
    fn handle(&mut self, _tag: &Handle, _printer: &mut StructuredPrinter) {}

    fn after_handle(&mut self, _printer: &mut StructuredPrinter) {}

    fn skip_descendants(&self) -> bool {
        true
    }
}

pub struct IgnoreFactory;
impl TagHandlerFactory for IgnoreFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(IgnoreHandler::default());
    }
}

#[derive(Default)]
pub struct CustomImgHandler {
    block_mode: bool,
}

impl TagHandler for CustomImgHandler {
    fn handle(&mut self, tag: &Handle, printer: &mut StructuredPrinter) {
        // hack: detect if the image has associated style and has display in block mode
        let style_tag = get_tag_attr(tag, "src");
        if let Some(style) = style_tag {
            if style.contains("display: block") {
                self.block_mode = true
            }
        }

        // skip avatar images
        if let Some(class) = get_tag_attr(tag, "class") {
            if class == "avatar" {
                return;
            }
        }

        printer.append_str("Image\n");
    }

    fn after_handle(&mut self, _printer: &mut StructuredPrinter) {}
}

pub struct CustomImgFactory;
impl TagHandlerFactory for CustomImgFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(CustomImgHandler::default());
    }
}

#[derive(Default)]
pub struct CustomQuoteHandler {
    start_pos: usize,
}

impl TagHandler for CustomQuoteHandler {
    fn handle(&mut self, _tag: &Handle, printer: &mut StructuredPrinter) {
        self.start_pos = printer.data.len();
    }

    fn after_handle(&mut self, printer: &mut StructuredPrinter) {
        let quote_content = &printer.data[self.start_pos..];
        let mut quoted = String::from("\n");

        for line in quote_content.lines() {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            quoted.push_str(&format!("> {trimmed}\n"));
        }

        printer.data.truncate(self.start_pos);
        printer.append_str(&quoted);
    }
}

pub struct CustomQuoteFactory;
impl TagHandlerFactory for CustomQuoteFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(CustomQuoteHandler::default());
    }
}

#[derive(Default)]
pub struct CustomAnchorHandler {
    start_pos: usize,
    url: String,
    emit_unchanged: bool,
    is_mention: bool,
}

fn clean_url(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if next.is_ascii_alphanumeric()
                    || matches!(
                        next,
                        '_' | '-' | '.' | '/' | '?' | '=' | '&' | '%' | '#' | ':' | '~'
                    )
                {
                    continue;
                }
            }
        }
        cleaned.push(ch);
    }

    cleaned
}

impl TagHandler for CustomAnchorHandler {
    fn handle(&mut self, tag: &Handle, printer: &mut StructuredPrinter) {
        if let Some(style) = get_tag_attr(tag, "class") {
            if style == "mention" {
                self.is_mention = true;
            }
        }

        if get_tag_attr(tag, "name").is_some() {
            self.emit_unchanged = true;
        }

        self.start_pos = printer.data.len();

        // try to extract a hyperlink
        self.url = match tag.data {
            NodeData::Element { ref attrs, .. } => {
                let attrs = attrs.borrow();
                let href = attrs
                    .iter()
                    .find(|attr| attr.name.local.to_string() == "href");
                match href {
                    Some(link) => link.value.to_string(),
                    None => String::new(),
                }
            }
            _ => String::new(),
        };
    }

    fn after_handle(&mut self, printer: &mut StructuredPrinter) {
        let end_pos = printer.data.len();
        let captured = &printer.data[self.start_pos..end_pos];
        let clean = clean_url(captured);
        if let Ok(url) = Url::parse(&clean) {
            if let Ok(other_url) = Url::parse(&self.url) {
                if url == other_url {
                    return;
                }
            }
        }
        if self.is_mention {
            return;
        }
        if !self.emit_unchanged {
            // add braces around already present text, put an url afterwards
            printer.insert_str(self.start_pos, "[");
            printer.append_str(&format!("]({})", self.url))
        }
    }
}

pub struct CustomAnchorFactory;
impl TagHandlerFactory for CustomAnchorFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(CustomAnchorHandler::default());
    }
}

pub struct DummyHandlerFactory;
impl TagHandlerFactory for DummyHandlerFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(DummyHandler::default());
    }
}

#[derive(Default)]
pub struct DetailsHandler {
    start_pos: usize,
}

impl TagHandler for DetailsHandler {
    fn handle(&mut self, _tag: &Handle, printer: &mut StructuredPrinter) {
        self.start_pos = printer.data.len();
    }

    fn after_handle(&mut self, printer: &mut StructuredPrinter) {
        let content = &printer.data[self.start_pos..].to_string();

        printer.data.truncate(self.start_pos);
        printer.append_str(&format!("||{}||", &content));
    }
}

pub struct DetailsFactory;
impl TagHandlerFactory for DetailsFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(DetailsHandler::default());
    }
}
#[derive(Default)]
pub struct AsideHandler {
    username: Option<String>,
}
impl TagHandler for AsideHandler {
    fn handle(&mut self, tag: &Handle, printer: &mut StructuredPrinter) {
        let mut custom: HashMap<String, Box<dyn TagHandlerFactory>> = HashMap::new();

        // if let Some(username) = get_tag_attr(tag, "data-username") {
        //     self.username = username;
        // }
        self.username = get_tag_attr(tag, "data-username");

        custom.insert(String::from("div"), Box::new(IgnoreFactory));
        custom.insert(String::from("img"), Box::new(CustomImgFactory));
        custom.insert(String::from("q"), Box::new(CustomQuoteFactory));
        custom.insert(String::from("cite"), Box::new(CustomQuoteFactory));
        custom.insert(String::from("quote"), Box::new(CustomQuoteFactory));
        custom.insert(String::from("a"), Box::new(CustomAnchorFactory));
        custom.insert(String::from("summary"), Box::new(DummyHandlerFactory));
        custom.insert(String::from("details"), Box::new(DetailsFactory));
        custom.insert(String::from("blockquote"), Box::new(CustomQuoteFactory));

        walk(tag, printer, &custom);
    }

    fn after_handle(&mut self, printer: &mut StructuredPrinter) {
        if let Some(username) = &self.username {
            printer.append_str(&format!("⤷ quoting: {}\n", username));
        }
    }

    fn skip_descendants(&self) -> bool {
        return true;
    }
}

pub struct AsideFactory;
impl TagHandlerFactory for AsideFactory {
    fn instantiate(&self) -> Box<dyn TagHandler> {
        return Box::new(AsideHandler::default());
    }
}

pub fn html_to_md(html: &str) -> String {
    let mut tag_factory: HashMap<String, Box<dyn TagHandlerFactory>> = HashMap::new();
    tag_factory.insert(String::from("img"), Box::new(CustomImgFactory));
    tag_factory.insert(String::from("blockquote"), Box::new(CustomQuoteFactory));
    tag_factory.insert(String::from("q"), Box::new(CustomQuoteFactory));
    tag_factory.insert(String::from("cite"), Box::new(CustomQuoteFactory));
    tag_factory.insert(String::from("quote"), Box::new(CustomQuoteFactory));
    tag_factory.insert(String::from("a"), Box::new(CustomAnchorFactory));
    tag_factory.insert(String::from("summary"), Box::new(DummyHandlerFactory));
    tag_factory.insert(String::from("details"), Box::new(DetailsFactory));
    tag_factory.insert(String::from("aside"), Box::new(AsideFactory));

    parse_html_custom(html, &tag_factory)
}
