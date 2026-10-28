//! Visible-text extraction from an HTML page.

use scraper::{ElementRef, Html, Selector};

/// Elements whose content is never visible page text.
const SKIPPED: &[&str] = &[
    "script", "style", "noscript", "nav", "header", "footer", "svg", "head", "template", "iframe",
];

/// Elements that separate words even without whitespace in the source.
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "li",
    "ul",
    "ol",
    "br",
    "tr",
    "td",
    "th",
    "table",
    "section",
    "article",
    "main",
    "aside",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "dt",
    "dd",
    "figure",
    "figcaption",
];

/// Visible text of `html` with whitespace collapsed, at most `max_chars` long.
/// Prefers `<main>` (then the longest `<article>`) over the whole `<body>`.
pub fn html_to_text(html: &str, max_chars: usize) -> String {
    let doc = Html::parse_document(html);
    truncate_chars(&main_text(&doc), max_chars)
}

fn main_text(doc: &Html) -> String {
    let main = matches(doc, "main")
        .into_iter()
        .map(text_of)
        .find(|t| !t.is_empty());
    if let Some(text) = main {
        return text;
    }
    let article = matches(doc, "article")
        .into_iter()
        .map(text_of)
        .max_by_key(String::len);
    if let Some(text) = article.filter(|t| !t.is_empty()) {
        return text;
    }
    let body = matches(doc, "body")
        .into_iter()
        .next()
        .unwrap_or_else(|| doc.root_element());
    text_of(body)
}

fn matches<'a>(doc: &'a Html, css: &str) -> Vec<ElementRef<'a>> {
    match Selector::parse(css) {
        Ok(sel) => doc.select(&sel).collect(),
        Err(_) => Vec::new(),
    }
}

enum Item<'a> {
    Element(ElementRef<'a>),
    Text(&'a str),
    Space,
}

/// Depth-first walk with an explicit stack (hostile pages can nest deeply).
fn text_of(root: ElementRef<'_>) -> String {
    let mut out = String::new();
    let mut stack = vec![Item::Element(root)];
    while let Some(item) = stack.pop() {
        match item {
            Item::Text(text) => out.push_str(text),
            Item::Space => out.push(' '),
            Item::Element(el) => {
                let name = el.value().name();
                if SKIPPED.contains(&name) {
                    continue;
                }
                let is_block = BLOCKS.contains(&name);
                if is_block {
                    out.push(' ');
                    stack.push(Item::Space);
                }
                push_children(&mut stack, el);
            }
        }
    }
    collapse_whitespace(&out)
}

/// Pushes children in reverse so they pop in document order.
fn push_children<'a>(stack: &mut Vec<Item<'a>>, el: ElementRef<'a>) {
    let children: Vec<_> = el.children().collect();
    for child in children.into_iter().rev() {
        if let Some(child_el) = ElementRef::wrap(child) {
            stack.push(Item::Element(child_el));
        } else if let Some(text) = child.value().as_text() {
            stack.push(Item::Text(text));
        }
    }
}

pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => s[..byte_idx].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_boilerplate_elements() {
        let html = r#"<html><head><title>T</title><style>p{}</style></head><body>
            <header>Site header</header><nav>Menu</nav>
            <script>var x = 1;</script><noscript>enable js</noscript>
            <p>Fireworks   at <b>midnight</b></p><svg><text>icon</text></svg>
            <footer>Copyright</footer></body></html>"#;
        assert_eq!(html_to_text(html, 1000), "Fireworks at midnight");
    }

    #[test]
    fn prefers_main_over_body() {
        let html =
            "<body><div>Sidebar ads</div><main><h1>Title</h1><p>Real content</p></main></body>";
        assert_eq!(html_to_text(html, 1000), "Title Real content");
    }

    #[test]
    fn falls_back_to_longest_article() {
        let html = "<body><div>x</div><article>short</article><article>this one is longer</article></body>";
        assert_eq!(html_to_text(html, 1000), "this one is longer");
    }

    #[test]
    fn block_elements_separate_words() {
        assert_eq!(
            html_to_text("<body><p>one</p><p>two</p>three<br>four</body>", 100),
            "one two three four"
        );
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(html_to_text("<body>héllo wörld</body>", 4), "héll");
        assert_eq!(html_to_text("<body>日本語のテキスト</body>", 3), "日本語");
        assert_eq!(html_to_text("<body>abc</body>", 100), "abc");
        assert_eq!(html_to_text("<body>abc</body>", 0), "");
    }

    #[test]
    fn empty_and_malformed_html_do_not_panic() {
        assert_eq!(html_to_text("", 10), "");
        assert_eq!(
            html_to_text("<div><p>unclosed <b>tags", 100),
            "unclosed tags"
        );
    }

    #[test]
    fn deeply_nested_html_does_not_overflow() {
        let html = format!("{}deep{}", "<div>".repeat(5000), "</div>".repeat(5000));
        assert!(html_to_text(&html, 100).contains("deep"));
    }
}
