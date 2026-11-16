//! MCP: let an AI agent on this computer (Claude Code, OpenCode, Cursor, a
//! browser agent…) do what this window does.
//!
//! One workspace: the switch and what the agent may do on the left, how to
//! connect each kind of agent on the right. The server (`paperloom-mcp`) is
//! a separate program that runs only between "Turn on" and "Turn off"; this
//! page starts and ends it and otherwise only reads `mcp.toml`.

use iced::widget::{button, column, container, row, scrollable, text, toggler, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Task};

use paperloom_common::mcp::{self, McpConfig};

use crate::pages::widgets_looks::blocking;
use crate::{theme, ui};

/// How to connect: one tab per kind of agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    AnyAgent,
    ClaudeCode,
    OpenCode,
    OtherClients,
    Browser,
    Activity,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::AnyAgent, "Any agent"),
    (Tab::ClaudeCode, "Claude Code"),
    (Tab::OpenCode, "OpenCode"),
    (Tab::OtherClients, "Cursor & others"),
    (Tab::Browser, "Browser agent"),
    (Tab::Activity, "Activity"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Changes,
    Remove,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    TurnOn,
    TurnOff,
    /// The server answered (or didn't) after Turn on / Turn off.
    Switched(Result<bool, String>),
    Allow(Permission, bool),
    TabSelected(Tab),
    Copy(String),
    NewToken,
    GuideLoaded(Option<String>),
}

pub struct State {
    running: bool,
    /// Turning on or off right now.
    busy: bool,
    /// Read when MCP is on (reading it creates the token, so not before).
    config: Option<McpConfig>,
    /// The full instructions for an agent, fetched from the running server.
    guide: Option<String>,
    tab: Tab,
    error: Option<String>,
    copied: bool,
    activity: String,
}

impl State {
    pub fn new() -> Self {
        let running = mcp::is_running();
        Self { running, busy: false, config: running.then(McpConfig::load), guide: None, tab: Tab::AnyAgent, error: None, copied: false, activity: String::new() }
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// The page was opened: see whether the server is still there.
    pub fn open(&mut self) -> Task<Message> {
        self.copied = false;
        self.check();
        self.activity = paperloom_common::log::tail("mcp", 40);
        self.load_guide()
    }

    /// A cheap look at whether the server still answers (it ends itself
    /// after 12 idle hours, or another window may have switched it off).
    pub fn check(&mut self) {
        if self.busy {
            return;
        }
        self.running = mcp::is_running();
        if self.running && self.config.is_none() {
            self.config = Some(McpConfig::load());
        }
        if !self.running {
            self.guide = None;
        }
    }

    fn load_guide(&self) -> Task<Message> {
        if self.running && self.guide.is_none() {
            blocking(mcp::guide, Message::GuideLoaded)
        } else {
            Task::none()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TurnOn => {
                self.busy = true;
                self.error = None;
                blocking(|| mcp::start().map(|()| true).map_err(|err| err.to_string()), Message::Switched)
            }
            Message::TurnOff => {
                self.busy = true;
                self.error = None;
                blocking(
                    || {
                        mcp::stop();
                        Ok(false)
                    },
                    Message::Switched,
                )
            }
            Message::Switched(result) => {
                self.busy = false;
                match result {
                    Ok(_) => self.error = None,
                    Err(why) => self.error = Some(format!("MCP could not be turned on: {why}")),
                }
                self.guide = None;
                self.check();
                if self.running {
                    self.config = Some(McpConfig::load());
                }
                self.load_guide()
            }
            Message::Allow(permission, on) => {
                let mut config = self.config.clone().unwrap_or_else(McpConfig::load);
                match permission {
                    Permission::Changes => config.allow_changes = on,
                    Permission::Remove => config.allow_remove = on,
                    Permission::Settings => config.allow_settings = on,
                }
                // The server reads the file on every call: this applies at once.
                let _ = config.save();
                self.config = Some(config);
                Task::none()
            }
            Message::TabSelected(tab) => {
                self.tab = tab;
                self.copied = false;
                if tab == Tab::Activity {
                    self.activity = paperloom_common::log::tail("mcp", 40);
                }
                Task::none()
            }
            Message::Copy(text) => {
                self.copied = true;
                iced::clipboard::write(text)
            }
            Message::NewToken => {
                let mut config = self.config.clone().unwrap_or_else(McpConfig::load);
                config.token = mcp::new_token();
                let _ = config.save();
                self.config = Some(config);
                self.guide = None;
                self.copied = false;
                self.load_guide()
            }
            Message::GuideLoaded(guide) => {
                self.guide = guide;
                Task::none()
            }
        }
    }

    pub fn header<'a>(&self) -> Element<'a, Message> {
        Space::new().into()
    }

    pub fn body<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        iced::widget::responsive(move |size| {
            let side = self.side(t);
            let main = self.main(t);
            if size.width >= 760.0 {
                row![container(side).width(Length::Fixed(300.0)).height(Length::Fill), container(main).width(Length::Fill).height(Length::Fill)].spacing(16).height(Length::Fill).into()
            } else {
                // Narrow window: the two panels stack and the page scrolls.
                ui::scroll_page(column![side, container(main).height(Length::Fixed(520.0))].spacing(16)).height(Length::Fill).into()
            }
        })
        .into()
    }

    /// Left: the switch, the address, what the agent may do.
    fn side<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let dot = |color: Color| container(Space::new().width(10).height(10)).style(move |_theme| container::Style { background: Some(Background::Color(color)), border: Border { radius: 5.0.into(), ..Border::default() }, ..container::Style::default() });
        let (state_text, state_color) = match (self.busy, self.running) {
            (true, false) => ("Turning on…", t.muted_foreground),
            (true, true) => ("Turning off…", t.muted_foreground),
            (false, true) => ("MCP is on", t.success),
            (false, false) => ("MCP is off", t.muted_foreground),
        };
        let switch: Element<'a, Message> = if self.running {
            button(ui::label("Turn off").size(14)).style(ui::secondary).padding([8, 16]).on_press_maybe((!self.busy).then_some(Message::TurnOff)).into()
        } else {
            button(ui::label("Turn on MCP").size(14)).style(ui::primary).padding([8, 16]).on_press_maybe((!self.busy).then_some(Message::TurnOn)).into()
        };

        let mut panel = column![
            row![dot(state_color), text(state_text).size(16).color(t.foreground)].spacing(8).align_y(Alignment::Center),
            text("An AI agent on this computer can see and change your wallpaper, library, collections, widgets and downloads, the same things this window does.").size(13).color(t.muted_foreground),
            switch,
        ]
        .spacing(12);

        if let Some(error) = &self.error {
            panel = panel.push(text(error.clone()).size(13).color(t.destructive));
        }

        if let (true, Some(config)) = (self.running, &self.config) {
            panel = panel.push(ui::rule(t));
            panel = panel.push(fact("Address", config.base_url(), t));
            panel = panel.push(fact("Reachable from", "This computer only".to_string(), t));
            panel = panel.push(ui::rule(t));
            panel = panel.push(text("What the agent may do").size(14).color(t.foreground));
            panel = panel.push(toggler(config.allow_changes).label("Allow changes").size(18).text_size(13).on_toggle(|on| Message::Allow(Permission::Changes, on)));
            panel = panel.push(note("Off: the agent can look but not change anything.", t));
            panel = panel.push(toggler(config.allow_remove).label("Allow removing").size(18).text_size(13).on_toggle_maybe(config.allow_changes.then_some(|on| Message::Allow(Permission::Remove, on))));
            panel = panel.push(note("Removing wallpapers and deleting collections.", t));
            panel = panel.push(toggler(config.allow_settings).label("Allow changing settings").size(18).text_size(13).on_toggle_maybe(config.allow_changes.then_some(|on| Message::Allow(Permission::Settings, on))));
            panel = panel.push(ui::rule(t));
            panel = panel.push(button(ui::label("New token").size(13)).style(ui::secondary).padding([6, 12]).on_press(Message::NewToken));
            panel = panel.push(note("Replaces the token. Agents you connected before need the new details.", t));
        } else {
            panel = panel.push(ui::rule(t));
            panel = panel.push(note("While off, nothing runs and nothing listens. MCP starts only from this button, never by itself, and turns itself off after 12 hours without a request.", t));
        }

        card(scrollable(panel.padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).height(Length::Fill).into(), t)
    }

    /// Right: how to connect each kind of agent.
    fn main<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let Some(config) = self.config.as_ref().filter(|_| self.running) else {
            let steps = column![
                text("How it works").font(theme::DISPLAY_FONT).size(18).color(t.foreground),
                text("1.  Turn on MCP.").size(14).color(t.foreground),
                text("2.  Copy the connection details for your agent: Claude Code, OpenCode, Cursor, a browser agent, or any other.").size(14).color(t.foreground),
                text("3.  Ask the agent in your own words: “download this video and set it as my wallpaper”, “make a collection of my rainy ones”, “put a clock in the top right corner”.").size(14).color(t.foreground),
                text("If Paperloom can't download from a link itself, the agent downloads the file and hands it over.").size(13).color(t.muted_foreground),
            ]
            .spacing(12);
            return card(steps.into(), t);
        };

        let tabs = ui::segmented(TABS.iter().map(|(tab, label)| ((*label).to_string(), self.tab == *tab, Message::TabSelected(*tab))).collect(), t);
        let (about, snippet): (&str, String) = match self.tab {
            Tab::AnyAgent => (
                "Paste this to any agent that runs on this computer. It tells the agent where Paperloom is and every way to talk to it.",
                self.guide.clone().unwrap_or_else(|| "Loading…".to_string()),
            ),
            Tab::ClaudeCode => ("Run this once in a terminal. Claude Code then has Paperloom's tools in every session.", claude_code(config)),
            Tab::OpenCode => ("Add this to opencode.json (in your project, or ~/.config/opencode/).", opencode(config)),
            Tab::OtherClients => ("For Cursor, Claude Desktop, VS Code, Windsurf and any other MCP client: add one of these to its MCP settings.", other_clients(config)),
            Tab::Browser => ("Give this to an agent that drives a web browser. The page has a form for every action.", browser(config)),
            Tab::Activity => ("What agents did most recently.", if self.activity.trim().is_empty() { "Nothing yet.".to_string() } else { self.activity.clone() }),
        };

        let mut bar = row![text(about).size(13).color(t.muted_foreground).width(Length::Fill)].spacing(12).align_y(Alignment::Center);
        if self.tab != Tab::Activity {
            bar = bar.push(button(ui::label(if self.copied { "Copied" } else { "Copy" }).size(13)).style(ui::primary).padding([7, 16]).on_press(Message::Copy(snippet.clone())));
        }
        let code = container(scrollable(container(text(snippet).size(12).font(Font::MONOSPACE).color(t.foreground)).padding(14).width(Length::Fill)).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(t.background)), border: Border { color: t.border, width: 1.0, radius: 4.0.into() }, ..container::Style::default() });

        card(column![scrollable(tabs).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::hidden())), bar, code].spacing(12).height(Length::Fill).into(), t)
    }
}

fn card<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(18)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: t.border, width: 1.0, radius: 6.0.into() }, ..container::Style::default() })
        .into()
}

fn note<'a>(content: &'a str, t: theme::Tokens) -> Element<'a, Message> {
    text(content).size(12).color(t.muted_foreground).into()
}

fn fact<'a>(name: &'a str, value: String, t: theme::Tokens) -> Element<'a, Message> {
    column![text(name).size(12).color(t.muted_foreground), text(value).size(13).font(Font::MONOSPACE).color(t.foreground)].spacing(2).into()
}

/// How an agent starts `paperloom-mcp` itself (stdin/stdout): the program
/// next to this one, or through Flatpak when that is how Paperloom is installed.
fn program() -> (String, Vec<String>) {
    match std::env::var("FLATPAK_ID") {
        Ok(app) => ("flatpak".to_string(), vec!["run".to_string(), "--command=paperloom-mcp".to_string(), app]),
        Err(_) => {
            let path = paperloom_common::process::sibling_binary("paperloom-mcp").map(|path| path.display().to_string()).unwrap_or_else(|| "paperloom-mcp".to_string());
            (path, Vec::new())
        }
    }
}

fn claude_code(config: &McpConfig) -> String {
    format!("claude mcp add --transport http paperloom {}/mcp --header \"Authorization: Bearer {}\"", config.base_url(), config.token)
}

fn opencode(config: &McpConfig) -> String {
    format!(
        "{{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"mcp\": {{\n    \"paperloom\": {{\n      \"type\": \"remote\",\n      \"url\": \"{}/mcp\",\n      \"enabled\": true,\n      \"headers\": {{ \"Authorization\": \"Bearer {}\" }}\n    }}\n  }}\n}}",
        config.base_url(),
        config.token
    )
}

fn other_clients(config: &McpConfig) -> String {
    let (command, args) = program();
    let quoted = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
    let args = args.iter().map(|arg| quoted(arg)).collect::<Vec<_>>().join(", ");
    format!(
        "Over HTTP (most clients):\n\n{{\n  \"mcpServers\": {{\n    \"paperloom\": {{\n      \"url\": \"{base}/mcp\",\n      \"headers\": {{ \"Authorization\": \"Bearer {token}\" }}\n    }}\n  }}\n}}\n\nA client that can't send headers:\n\n  {base}/mcp?token={token}\n\nA client that starts a program instead (stdio):\n\n{{\n  \"mcpServers\": {{\n    \"paperloom\": {{\n      \"command\": {command},\n      \"args\": [{args}]\n    }}\n  }}\n}}",
        base = config.base_url(),
        token = config.token,
        command = quoted(&command),
    )
}

fn browser(config: &McpConfig) -> String {
    format!(
        "Open {base}/?token={token} in your browser. It is the control page of Paperloom, the live-wallpaper app on this computer. Every action is a form on that page: fill in what it asks for, press its button, and read the result on the page that follows. Start with “status”.",
        base = config.base_url(),
        token = config.token
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_carry_the_address_and_token() {
        let config = McpConfig { port: 47821, token: "0123456789abcdef0123456789abcdef".into(), ..McpConfig::default() };
        for snippet in [claude_code(&config), opencode(&config), other_clients(&config), browser(&config)] {
            assert!(snippet.contains("http://127.0.0.1:47821"), "{snippet}");
            assert!(snippet.contains(&config.token), "{snippet}");
        }
        assert!(opencode(&config).contains("\"type\": \"remote\""));
    }
}
