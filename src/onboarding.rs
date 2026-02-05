//! HardClaw Onboarding TUI
//!
//! Interactive terminal application for:
//! - Creating/loading wallets
//! - Running a node
//! - Mining the genesis block

use std::io::{self, stdout};
use std::time::Duration;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};

use hardclaw::{types::Block as HcBlock, wallet::Wallet, verifier::EnvironmentCheck};

/// Application state
enum AppState {
    Welcome,
    MainMenu,
    CreateWallet,
    WalletCreated {
        address: String,
        path: String,
        seed_phrase: String,
    },
    LoadWallet,
    WalletLoaded {
        address: String,
    },
    EnvironmentSetup,
    EnvironmentChecked {
        checks: Vec<EnvironmentCheck>,
    },
    RunNode,
    GenesisMined {
        block_hash: String,
    },
    #[allow(dead_code)] // Planned feature - node integration
    NodeRunning,
    Help,
    Quit,
}

/// Menu selection state
struct MenuState {
    items: Vec<&'static str>,
    selected: usize,
}

impl MenuState {
    fn new(items: Vec<&'static str>) -> Self {
        Self { items, selected: 0 }
    }

    fn next(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }

    fn previous(&mut self) {
        self.selected = self.selected.checked_sub(1).unwrap_or(self.items.len() - 1);
    }
}

/// Main application
struct App {
    state: AppState,
    menu: MenuState,
    wallet: Option<Wallet>,
    genesis_block: Option<HcBlock>,
    message: Option<String>,
}

impl App {
    fn new() -> Self {
        let menu_items = if Wallet::default_exists() {
            vec![
                "Load Wallet",
                "Create New Wallet",
                "Check Verification Environment",
                "Run Verifier Node",
                "Help",
                "Quit",
            ]
        } else {
            vec![
                "Create Wallet",
                "Load Wallet",
                "Check Verification Environment",
                "Run Verifier Node",
                "Help",
                "Quit",
            ]
        };

        Self {
            state: AppState::Welcome,
            menu: MenuState::new(menu_items),
            wallet: None,
            genesis_block: None,
            message: None,
        }
    }

    fn handle_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        // Ctrl+C always quits
        if modifiers.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('c') {
            return true;
        }

        match &self.state {
            AppState::Welcome => {
                self.state = AppState::MainMenu;
            }
            AppState::MainMenu => match key {
                KeyCode::Up | KeyCode::Char('k') => self.menu.previous(),
                KeyCode::Down | KeyCode::Char('j') => self.menu.next(),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.handle_menu_selection();
                }
                KeyCode::Char('q') => return true,
                _ => {}
            },
            AppState::CreateWallet => match key {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.create_wallet();
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::WalletCreated { .. } => {
                self.state = AppState::MainMenu;
            }
            AppState::LoadWallet => match key {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.load_wallet();
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::WalletLoaded { .. } => {
                self.state = AppState::MainMenu;
            }
            AppState::EnvironmentSetup => {
                // Shouldn't receive input here
            }
            AppState::EnvironmentChecked { .. } => {
                self.state = AppState::MainMenu;
            }
            AppState::RunNode => match key {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.mine_genesis();
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::GenesisMined { .. } => {
                self.state = AppState::MainMenu;
            }
            AppState::Help => {
                self.state = AppState::MainMenu;
            }
            AppState::NodeRunning => match key {
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::Quit => return true,
        }

        false
    }

    fn handle_menu_selection(&mut self) {
        let selected = self.menu.items[self.menu.selected];
        match selected {
            "Create Wallet" | "Create New Wallet" => {
                self.state = AppState::CreateWallet;
            }
            "Load Wallet" => {
                self.state = AppState::LoadWallet;
            }
            "Check Verification Environment" => {
                self.check_environment();
            }
            "Run Verifier Node" => {
                if self.wallet.is_none() {
                    self.message = Some("Please create or load a wallet first".to_string());
                } else {
                    self.state = AppState::RunNode;
                }
            }
            "Help" => {
                self.state = AppState::Help;
            }
            "Quit" => {
                self.state = AppState::Quit;
            }
            _ => {}
        }
    }

    fn create_wallet(&mut self) {
        // Generate wallet with BIP39 mnemonic
        let mnemonic = hardclaw::generate_mnemonic();
        let seed_phrase = mnemonic.to_string();
        let keypair = hardclaw::keypair_from_mnemonic(&mnemonic, "");

        // Create wallet from the keypair
        let secret_bytes = keypair.secret_key().to_bytes();
        let mut wallet =
            Wallet::from_secret_bytes(secret_bytes).expect("valid keypair from mnemonic");
        let address = wallet.address().to_string();

        match wallet.save_as_default() {
            Ok(()) => {
                let path = Wallet::default_path().display().to_string();
                self.wallet = Some(wallet);
                self.message = None;
                self.state = AppState::WalletCreated {
                    address,
                    path,
                    seed_phrase,
                };
                // Update menu to show "Load Wallet" as first option now
                self.menu = MenuState::new(vec![
                    "Load Wallet",
                    "Create New Wallet",
                    "Check Verification Environment",
                    "Run Verifier Node",
                    "Help",
                    "Quit",
                ]);
            }
            Err(e) => {
                self.message = Some(format!("Failed to save wallet: {}", e));
                self.state = AppState::MainMenu;
            }
        }
    }

    fn load_wallet(&mut self) {
        match Wallet::load_default() {
            Ok(wallet) => {
                let address = wallet.address().to_string();
                self.wallet = Some(wallet);
                self.message = None;
                self.state = AppState::WalletLoaded { address };
            }
            Err(e) => {
                self.message = Some(format!("Failed to load wallet: {}", e));
                self.state = AppState::MainMenu;
            }
        }
    }

    fn mine_genesis(&mut self) {
        if let Some(wallet) = &self.wallet {
            let genesis = HcBlock::genesis(*wallet.keypair().public_key());
            let block_hash = genesis.hash.to_hex();
            self.genesis_block = Some(genesis);
            self.message = None;
            self.state = AppState::GenesisMined { block_hash };
        }
    }

    fn check_environment(&mut self) {
        let checks = EnvironmentCheck::check_all();
        self.state = AppState::EnvironmentChecked { checks };
    }

    fn ui(&self, frame: &mut Frame) {
        let size = frame.area();

        // Main layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(10),   // Content
                Constraint::Length(3), // Footer
            ])
            .split(size);

        // Header
        self.render_header(frame, chunks[0]);

        // Content based on state
        match &self.state {
            AppState::Welcome => self.render_welcome(frame, chunks[1]),
            AppState::MainMenu => self.render_main_menu(frame, chunks[1]),
            AppState::CreateWallet => self.render_create_wallet(frame, chunks[1]),
            AppState::WalletCreated {
                address,
                path,
                seed_phrase,
            } => {
                self.render_wallet_created(frame, chunks[1], address, path, seed_phrase);
            }
            AppState::LoadWallet => self.render_load_wallet(frame, chunks[1]),
            AppState::WalletLoaded { address } => {
                self.render_wallet_loaded(frame, chunks[1], address);
            }
            AppState::EnvironmentSetup => self.render_environment_setup(frame, chunks[1]),
            AppState::EnvironmentChecked { checks } => {
                self.render_environment_checked(frame, chunks[1], checks);
            }
            AppState::RunNode => self.render_run_node(frame, chunks[1]),
            AppState::GenesisMined { block_hash } => {
                self.render_genesis_mined(frame, chunks[1], block_hash);
            }
            AppState::Help => self.render_help(frame, chunks[1]),
            AppState::NodeRunning => self.render_node_running(frame, chunks[1]),
            AppState::Quit => {}
        }

        // Footer
        self.render_footer(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let wallet_status = if let Some(wallet) = &self.wallet {
            format!(" | Wallet: {}...", &wallet.address().to_string()[..16])
        } else {
            String::new()
        };

        let header = Paragraph::new(format!("HardClaw v{}{}", hardclaw::VERSION, wallet_status))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(header, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let hint = match &self.state {
            AppState::Welcome => "Press any key to continue",
            AppState::MainMenu => "j/k: Navigate | Enter: Select | q: Quit",
            AppState::CreateWallet | AppState::LoadWallet | AppState::RunNode => {
                "Enter/y: Confirm | Esc/n: Cancel"
            }
            AppState::WalletCreated { .. }
            | AppState::WalletLoaded { .. }
            | AppState::GenesisMined { .. }
            | AppState::EnvironmentChecked { .. }
            | AppState::Help => "Press any key to continue",
            AppState::EnvironmentSetup => "Checking environment...",
            AppState::NodeRunning => "q: Stop node | Ctrl+C: Force quit",
            AppState::Quit => "",
        };

        let mut footer_text = hint.to_string();
        if let Some(msg) = &self.message {
            footer_text = format!("{} | {}", msg, hint);
        }

        let footer = Paragraph::new(footer_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(footer, area);
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let logo = r#"

    ██╗  ██╗ █████╗ ██████╗ ██████╗  ██████╗██╗      █████╗ ██╗    ██╗
    ██║  ██║██╔══██╗██╔══██╗██╔══██╗██╔════╝██║     ██╔══██╗██║    ██║
    ███████║███████║██████╔╝██║  ██║██║     ██║     ███████║██║ █╗ ██║
    ██╔══██║██╔══██║██╔══██╗██║  ██║██║     ██║     ██╔══██║██║███╗██║
    ██║  ██║██║  ██║██║  ██║██████╔╝╚██████╗███████╗██║  ██║╚███╔███╔╝
    ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝

           Proof-of-Verification for the Autonomous Agent Economy

                        "We do not trust; we verify."
"#;

        let welcome = Paragraph::new(logo)
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center)
            .block(Block::default());

        frame.render_widget(welcome, area);
    }

    fn render_main_menu(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ])
            .split(area);

        let items: Vec<ListItem> = self
            .menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let style = if i == self.menu.selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let prefix = if i == self.menu.selected { "> " } else { "  " };
                ListItem::new(format!("{}{}", prefix, item)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Main Menu ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));

        frame.render_widget(list, chunks[1]);
    }

    fn render_create_wallet(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Create New Wallet",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("This will generate a new Ed25519 keypair for your HardClaw wallet."),
            Line::from(""),
            Line::from("The wallet will be saved to:"),
            Line::from(Span::styled(
                format!("  {}", Wallet::default_path().display()),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "WARNING: Keep your wallet file secure!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from("Anyone with access to this file can control your funds."),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "Create wallet? (y/n)",
                Style::default().fg(Color::Green),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(60, 60, area));
    }

    fn render_wallet_created(
        &self,
        frame: &mut Frame,
        area: Rect,
        address: &str,
        path: &str,
        seed_phrase: &str,
    ) {
        let words: Vec<&str> = seed_phrase.split_whitespace().collect();
        let mut text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "🔐 WALLET CREATED - SAVE YOUR SEED PHRASE! 🔐",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Write down these 24 words in order and store them securely.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                "This is the ONLY way to recover your wallet!",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Display seed phrase in 4 columns of 6 words
        for row in 0..6 {
            let mut line_spans = vec![];
            for col in 0..4 {
                let idx = col * 6 + row;
                if idx < words.len() {
                    line_spans.push(Span::styled(
                        format!("{:2}. {:<11} ", idx + 1, words[idx]),
                        Style::default().fg(Color::Cyan),
                    ));
                }
            }
            text.push(Line::from(line_spans));
        }

        text.extend(vec![
            Line::from(""),
            Line::from(Span::styled("Address:", Style::default().fg(Color::White))),
            Line::from(Span::styled(
                address.to_string(),
                Style::default().fg(Color::Green),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("Wallet saved to: {}", path),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to continue...",
                Style::default().fg(Color::DarkGray),
            )),
        ]);

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        );

        frame.render_widget(paragraph, centered_rect(85, 90, area));
    }

    fn render_load_wallet(&self, frame: &mut Frame, area: Rect) {
        let default_path = Wallet::default_path();
        let exists = default_path.exists();

        let text = if exists {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Load Existing Wallet",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Found wallet at:"),
                Line::from(Span::styled(
                    format!("  {}", default_path.display()),
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled(
                    "Load this wallet? (y/n)",
                    Style::default().fg(Color::Green),
                )),
            ]
        } else {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No Wallet Found",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("No wallet exists at:"),
                Line::from(Span::styled(
                    format!("  {}", default_path.display()),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from("Please create a new wallet first."),
                Line::from(""),
                Line::from(Span::styled(
                    "Press any key to go back...",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        };

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(60, 50, area));
    }

    fn render_wallet_loaded(&self, frame: &mut Frame, area: Rect, address: &str) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Wallet Loaded Successfully!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Wallet address:"),
            Line::from(Span::styled(
                address.to_string(),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to continue...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );

        frame.render_widget(paragraph, centered_rect(60, 40, area));
    }

    fn render_run_node(&self, frame: &mut Frame, area: Rect) {
        let wallet_addr = self
            .wallet
            .as_ref()
            .map(|w| w.address().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Run Verifier Node",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("This will start a verifier node on the HardClaw network."),
            Line::from(""),
            Line::from("Your node will:"),
            Line::from(Span::styled(
                "  • Verify solutions and earn rewards",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  • Participate in consensus",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  • Help secure the network",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from("Node address:"),
            Line::from(Span::styled(
                format!("  {}...", &wallet_addr[..32.min(wallet_addr.len())]),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Note: Use 'hardclaw-node --verifier' to run a full node",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "Start node? (y/n)",
                Style::default().fg(Color::Green),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(70, 75, area));
    }

    fn render_genesis_mined(&self, frame: &mut Frame, area: Rect, block_hash: &str) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "GENESIS BLOCK MINED!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "The HardClaw network has begun.",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from("Block Hash:"),
            Line::from(Span::styled(
                format!("  {}", block_hash),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Block Details:"),
            Line::from(Span::styled(
                "  Height:     0",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                "  Timestamp:  Now",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                "  Txns:       0",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "\"We do not trust; we verify.\"",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::ITALIC),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to continue...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );

        frame.render_widget(paragraph, centered_rect(70, 70, area));
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "HardClaw - Quick Start Guide",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "1. Create a Wallet",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Generate a new Ed25519 keypair to identify your node."),
            Line::from("   Your wallet is your identity on the network."),
            Line::from(""),
            Line::from(Span::styled(
                "2. Run Verifier Node",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Start your node to verify solutions and earn rewards."),
            Line::from("   Use 'hardclaw-node --verifier' from the command line."),
            Line::from(""),
            Line::from(Span::styled(
                "3. Participate in the Network",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Submit jobs, verify solutions, and produce blocks."),
            Line::from("   Earn HCLAW rewards for honest verification."),
            Line::from(""),
            Line::from(Span::styled(
                "Protocol Roles:",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Requester - Submit jobs with bounties"),
            Line::from("   Solver    - Execute work, submit solutions"),
            Line::from("   Verifier  - Verify solutions, mine blocks"),
            Line::from(""),
            Line::from(Span::styled(
                "Fee Distribution: ",
                Style::default().fg(Color::White),
            )),
            Line::from("   95% Solver | 4% Verifier | 1% Burned"),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to return...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, centered_rect(80, 85, area));
    }

    fn render_node_running(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Node Running",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Status: Active"),
            Line::from("Peers: 0"),
            Line::from("Height: 1"),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'q' to stop...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );

        frame.render_widget(paragraph, centered_rect(50, 40, area));
    }

    fn render_environment_setup(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(Span::styled(
                "🔍 Checking Verification Environment",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Please wait..."),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(60, 30, area));
    }

    fn render_environment_checked(&self, frame: &mut Frame, area: Rect, checks: &[EnvironmentCheck]) {
        let mut text = vec![
            Line::from(Span::styled(
                "🔍 Verification Environment Status",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        for check in checks {
            let (status, color) = if check.available {
                ("✓", Color::Green)
            } else {
                ("✗", Color::Red)
            };

            text.push(Line::from(vec![
                Span::styled(format!("[{}] ", status), Style::default().fg(color)),
                Span::styled(
                    check.language.display_name(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(": "),
                Span::raw(check.version.as_deref().unwrap_or("not available")),
            ]));

            // Show warnings
            for warning in &check.warnings {
                text.push(Line::from(Span::styled(
                    format!("    ⚠ {}", warning),
                    Style::default().fg(Color::Yellow),
                )));
            }

            // Show setup instructions if not available
            if let Some(instructions) = &check.setup_instructions {
                text.push(Line::from(""));
                for line in instructions.lines() {
                    text.push(Line::from(Span::styled(
                        format!("    {}", line),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }

            text.push(Line::from(""));
        }

        // Summary
        let available_count = checks.iter().filter(|c| c.available).count();
        let total_count = checks.len();

        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            format!("Supported Languages: {}/{}", available_count, total_count),
            Style::default()
                .fg(if available_count == total_count {
                    Color::Green
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD),
        )));

        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            "Press any key to return to menu",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Environment Check "),
            );

        frame.render_widget(paragraph, centered_rect(80, 80, area));
    }
}

/// Helper function to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn main() -> io::Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new();

    // Main loop
    loop {
        terminal.draw(|f| app.ui(f))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_input(key.code, key.modifiers) {
                    break;
                }
            }
        }
    }

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;

    // Print farewell
    println!("\n  Thanks for using HardClaw!");
    println!("  \"We do not trust; we verify.\"\n");

    Ok(())
}
