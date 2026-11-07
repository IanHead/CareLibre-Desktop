pub mod contact_editor;
pub mod contacts;
pub mod home;
pub mod intake;
pub mod placeholder;
pub mod relationships;
pub mod settings;

use crate::theme::ThemeMode;

/// A user intent raised by a page, handled by the application shell.
pub enum Action {
    Navigate(Page),
    Notice(String),
    SaveServer {
        base_url: String,
    },
    TestServer,
    Login {
        email: String,
        password: String,
        organisation_slug: Option<String>,
    },
    Register {
        organisation_name: String,
        organisation_slug: String,
        full_name: String,
        email: String,
        password: String,
    },
    Logout,
    SetClientSideDecorations(bool),
    SetRememberLogin(bool),
    SetTheme(ThemeMode),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Home,
    Intake,
    Contacts,
    Clients,
    Workers,
    Rostering,
    Relationships,
    Services,
    Documents,
    Communications,
    Reports,
    Settings,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Intake => "Intake",
            Self::Contacts => "Contacts",
            Self::Clients => "Clients",
            Self::Workers => "Workers",
            Self::Rostering => "Rostering",
            Self::Relationships => "Relationships",
            Self::Services => "Services & rates",
            Self::Documents => "Documents",
            Self::Communications => "Communications",
            Self::Reports => "Reports",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "🏠",
            Self::Intake => "📝",
            Self::Contacts => "👥",
            Self::Clients => "👤",
            Self::Workers => "👷",
            Self::Rostering => "📅",
            Self::Relationships => "🔗",
            Self::Services => "🏷",
            Self::Documents => "📄",
            Self::Communications => "✉",
            Self::Reports => "📊",
            Self::Settings => "⚙",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Home => "Your care operation at a glance.",
            Self::Intake => "Guided onboarding for clients, workers and other contacts.",
            Self::Contacts => "All contacts across clients, workers and support networks.",
            Self::Clients => "Participant and client records, contacts and care plans.",
            Self::Workers => "Support worker records, availability, skills and compliance.",
            Self::Rostering => "Shifts, client and worker assignments, and sessions.",
            Self::Relationships => "Two-sided links between contacts and organisations.",
            Self::Services => "Funded services, rate cards and effective-dated rate versions.",
            Self::Documents => "Merge templates for quotes, service agreements and reports.",
            Self::Communications => "Configurable SMS, email and other external channels.",
            Self::Reports => "Operational and revenue reporting, with Power BI export.",
            Self::Settings => "Organisation, divisions, cost centres and configuration.",
        }
    }

    pub fn features(self) -> &'static [&'static str] {
        match self {
            Self::Home => &[],
            Self::Intake => &[
                "Scenario picker with persisted drafts",
                "Client intake with health, support network and behaviours",
                "Worker intake with position, employment terms and award details",
                "Other contacts such as coordinators, family and health professionals",
            ],
            Self::Contacts => &[
                "One directory for clients, workers and dual-role participants",
                "Support contacts, coordinators, family and health professionals",
                "Instant local search across names, email and phone",
            ],
            Self::Clients => &[
                "Unified contact records for clients, workers and dual-role participants",
                "Next-of-kin, representatives and consent management",
                "Custom fields and form layouts per organisation",
                "NDIS and Aged Care plan and funding details",
            ],
            Self::Workers => &[
                "Skills, checks and credential expiry tracking",
                "Availability and preferred client matching",
                "Worker assignment history and utilisation",
                "Mobile access controls per field",
            ],
            Self::Relationships => &[
                "Link any contact to any contact or organisation",
                "A role for each side of the relationship",
                "Delete links without touching either record",
            ],
            Self::Rostering => &[
                "Shifts with client and worker assignments or sessions",
                "Copy and paste shifts across days and workers",
                "Opening-hours shifts for properties and respite programs",
                "Cost-centre inheritance through the roster hierarchy",
            ],
            Self::Services => &[
                "Services mapped to NDIS or Aged Care funding codes",
                "Multiple rates per service with units of measure",
                "Effective-dated rate versions (e.g. 1 July increases)",
                "Quote generation from natural-language requests",
            ],
            Self::Documents => &[
                "Customisable document merge templates",
                "Quotes, service agreements and progress reports",
                "Per-organisation branding and clause libraries",
            ],
            Self::Communications => &[
                "Configurable SMS and email providers",
                "Template-driven appointment and shift notifications",
                "Delivery history and opt-out handling",
            ],
            Self::Reports => &[
                "Revenue, utilisation and workforce dashboards",
                "Scheduled exports to Power BI",
                "Xero, QuickBooks and Salesforce integrations",
            ],
            Self::Settings => &[
                "Divisions, cost centres and custom inheritance rules",
                "Database connection and backup settings",
                "Role-based access and audit logging",
            ],
        }
    }
}
