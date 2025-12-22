use discourse::{
    bundle::PostData,
    model::{PostId, post::Post},
};
use once_cell::sync::Lazy;
use pulsar::{DeserializeMessage, Error as PulsarError, Payload, SerializeMessage};
use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serenity::all::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, MessageId};

use crate::{md::html_to_md, utils::trim_to_n_chars};

#[derive(Serialize, Deserialize, Debug)]
pub struct DiscordMapping {
    pub discord_message_id: MessageId,
    pub post_id: PostId,
}

impl SerializeMessage for DiscordMapping {
    fn serialize_message(input: Self) -> Result<pulsar::producer::Message, PulsarError> {
        let payload = serde_json::to_vec(&input).map_err(|e| PulsarError::Custom(e.to_string()))?;

        Ok(pulsar::producer::Message {
            payload,
            ..Default::default()
        })
    }
}

impl DeserializeMessage for DiscordMapping {
    type Output = Result<DiscordMapping, serde_json::Error>;

    fn deserialize_message(payload: &Payload) -> Self::Output {
        serde_json::from_slice(&payload.data)
    }
}

fn get_normal_description(post_data: &PostData) -> String {
    let mut ret = String::new();
    let mut reply = false;
    if let Some(replying_to) = &post_data.replying_to_post {
        reply = true;
        let username = &replying_to.username;
        let html = &replying_to.cooked;
        let md = html_to_md(html);
        let mut quote = String::default();
        for line in md.lines() {
            let quoted = format!("> {line}\n");
            quote.push_str(&quoted);
        }
        let quote = trim_to_n_chars(&quote, 1000);
        ret.push_str(&quote);
        if !quote.ends_with("\n") {
            ret.push('\n');
        }
        ret.push_str(&format!("⤷ replying to: {username}\n\n"));
    }

    let html = &post_data.post.cooked;
    let md = html_to_md(html);
    let md = trim_to_n_chars(&md, if reply { 900 } else { 1900 });
    ret.push_str(&md);
    ret
}

fn get_admin_action_description(post_data: &PostData) -> String {
    match post_data.post.action_code.as_deref() {
        Some(code) => match code {
            "public_open" => String::from("Made this topic public"),
            "open_topic" => String::from("Converted this to a topic"),
            "private_topic" => String::from("Made this topic a personal message"),
            "split_topic" => String::from("Split this topic"),
            "invited_user" => match post_data.post.action_code_who.as_deref() {
                Some(who) => format!("Invited {}", who),
                None => String::from("Invited a user"),
            },
            "invited_group" => match post_data.post.action_code_who.as_deref() {
                Some(who) => format!("Invited group {}", who),
                None => String::from("Invited a group"),
            },
            "user_left" => match post_data.post.action_code_who.as_deref() {
                Some(who) => format!("{} removed themselves from this message", who),
                None => String::from("A user removed themselves from this message"),
            },
            "removed_user" => match post_data.post.action_code_who.as_deref() {
                Some(who) => format!("Removed {}", who),
                None => String::from("Removed a user"),
            },
            "removed_group" => match post_data.post.action_code_who.as_deref() {
                Some(who) => format!("Removed {} group", who),
                None => String::from("Removed a group"),
            },
            "autobumped" => String::from("Automatically bumped"),
            "tags_changed" => String::from("Tags updated"),
            "category_changed" => String::from("Category updated"),
            "autoclosed.enabled" | "closed.enabled" => String::from("Closed"),
            "autoclosed.disabled" | "closed.disabled" => String::from("Opened"),
            "archived.enabled" => String::from("Archived"),
            "archived.disabled" => String::from("Unarchived"),
            "pinned.enabled" => String::from("Pinned"),
            "pinned.disabled" | "pinned_globally.disabled" => String::from("Unpinned"),
            "pinned_globally.enabled" => String::from("Pinned globally"),
            "visible.enabled" => String::from("Listed"),
            "visible.disabled" => String::from("Unlisted"),
            "banner.enabled" => String::from(
                "Made this a banner. It will appear at the top of every page until it is dismissed by the user.",
            ),
            "banner.disabled" => String::from(
                "Removed this banner. It will no longer appear at the top of every page.",
            ),
            "forwarded" => String::from("Forwarded the above email"),
            _ => String::from(""),
        },
        None => String::from(""),
    }
}

pub fn get_post_content(post_data: &PostData) -> String {
    match post_data.post.post_type {
        3 => {
            let raw = get_admin_action_description(post_data);
            format!("*{}*", raw)
        }
        _ => get_normal_description(post_data),
    }
}

pub fn create_embed_author(post: &Post, base_url: &str) -> (String, String) {
    let name = &post.username;
    let avatar = format!(
        "{}/{}",
        base_url,
        post.avatar_template.replace("{size}", "144")
    );
    (name.clone(), avatar)
}

pub fn get_images(post: &Post, url: &str) -> Vec<CreateEmbed> {
    let mut ret = Vec::new();
    let raw = &post.cooked;
    let images = extract_imgs_excluding_class(&raw, "avatar");
    for (i, image) in images.iter().enumerate() {
        if i >= 9 {
            break;
        }
        let e = CreateEmbed::new().url(url).image(image);
        ret.push(e);
    }
    ret
}

pub fn get_link(post_data: &PostData, base_url: &str) -> Option<String> {
    let url = format!(
        "{base_url}/t/{}/{}",
        post_data.topic.id, post_data.post.post_number
    );
    Some(url)
}

pub fn get_title(post_data: &PostData) -> Option<String> {
    let thread_name = &post_data.topic.title;
    let ordinal = post_data.post.post_number;
    Some(format!("{thread_name} #{ordinal}"))
}

pub fn create_embeds(post_data: &PostData) -> Option<Vec<CreateEmbed>> {
    let base_url = &post_data.base_url;
    let mut ret: Vec<CreateEmbed> = Vec::new();
    let url = get_link(&post_data, base_url)?;
    let media = get_images(&post_data.post, &url);

    let color = _hex_color_to_int(&post_data.category.color)?;
    let description = get_post_content(&post_data);
    let title = get_title(&post_data)?;
    let author_name = &post_data.post.display_username;
    let username = &post_data.post.username;
    let author_url = format!("{base_url}/u/{username}");
    let icon_url = {
        let ret = post_data.post.avatar_template.replace("{size}", "144");
        Some(format!("{base_url}/{ret}"))
    }?;
    let author = CreateEmbedAuthor::new(author_name)
        .icon_url(icon_url)
        .url(author_url);
    let timestamp = post_data.post.created_at;
    let embed = CreateEmbed::new()
        .description(description)
        .url(url)
        .title(title)
        .author(author)
        .color(color)
        .timestamp(timestamp);
    ret.push(embed);

    for image in media {
        // println!("found image");
        ret.push(image);
    }

    Some(ret)
}

pub fn create_embeds_impersonate(post_data: &PostData, base_url: &str) -> Vec<CreateEmbed> {
    let mut ret: Vec<CreateEmbed> = Vec::new();
    if let Some(url) = get_link(&post_data, base_url) {
        let media = get_images(&post_data.post, &url);
        let description = get_post_content(&post_data);
        let ordinal = post_data.post.post_number;
        let footer = CreateEmbedFooter::new(&post_data.post.username);
        let mut embed = CreateEmbed::new()
            .description(description)
            .url(url)
            .title(format!("{ordinal}"))
            .footer(footer)
            .timestamp(post_data.post.updated_at);
        if post_data.post.post_type == 2 {
            embed = embed.color(_hex_color_to_int("#0277BD").unwrap());
        }
        ret.push(embed);

        for image in media {
            // println!("found image");
            ret.push(image);
        }
    }
    ret
}

pub fn _hex_color_to_int(hex: &str) -> Option<u32> {
    // Remove leading '#' if present
    let hex = hex.strip_prefix('#').unwrap_or(hex);

    if hex.len() != 6 {
        return None;
    }

    u32::from_str_radix(hex, 16).ok()
}

pub fn extract_imgs_excluding_class(html: &str, excluded_class: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let img_selector = Selector::parse("img").unwrap();

    document
        .select(&img_selector)
        .filter(|img| {
            match img.value().attr("class") {
                Some(class_list) => {
                    // Split class attribute by whitespace and check for excluded class
                    !class_list.split_whitespace().any(|c| c == excluded_class)
                }
                None => true, // No class attribute, so keep it
            }
        })
        .filter_map(|img| img.value().attr("src").map(String::from))
        .collect()
}

pub fn _tidy_description(input: &mut String) {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\[.*?\]\(.*?\)").unwrap());

    // let re = Regex::new(r"!\[.*?\]\(.*?\)").unwrap();
    let replaced = RE.replace_all(input, "Media");
    let truncated = replaced.chars().take(4000).collect::<String>();
    *input = truncated;
}

pub async fn _create_embeds(
    base_url: &str,
    post: &Value,
    replying_to_post: &Option<Value>,
    topic: &Value,
    category: &Value,
) -> Option<Vec<Value>> {
    let color = _hex_color_to_int(category.get("category")?.get("color")?.as_str()?)?;
    let url = format!(
        "{base_url}/t/{}/{}",
        topic.get("id")?,
        post.get("post_number")?
    );
    let mut raw = String::from(post.get("raw")?.as_str()?);
    _tidy_description(&mut raw);
    if let Some(replying_post) = replying_to_post {
        println!(
            "Post {} is replying to post {}",
            post["id"], replying_post["id"]
        );
        // TODO
        let mut replying_raw = replying_post["raw"].as_str().unwrap().to_string();
        _tidy_description(&mut replying_raw);
        // raw = String::from("Replying to post:\n") + raw;
        let mut new_raw = String::from("Replying to post:\n");
        new_raw.push_str(&raw);
        raw = new_raw;
    }
    let mut ret = Vec::new();
    let pfp = post
        .get("avatar_template")?
        .as_str()?
        .to_string()
        .replace("{size}", "144");
    let main_json = json!({
        "title": format!("{} #{}", topic.get("title")?.as_str()?, post.get("post_number")?),
        "url": url,
        "description": raw,
        "color": color,
        "timestamp": post.get("created_at"),
        "author": {
            "name": post.get("display_username"),
            "url": format!("{base_url}/u/{}", String::from(post.get("username")?.as_str()?)),
            "icon_url": format!("{base_url}{}", pfp),
        }
    });
    ret.push(main_json);
    let html = post.get("cooked")?.as_str()?;
    let media = extract_imgs_excluding_class(html, &"avatar");
    let slice = &media[..media.len().min(9)];

    for img in slice {
        ret.push(json!({
            "url": url,
            "image": {
                "url": img
            }
        }));
    }

    Some(ret)
}
