use egui_lucide::Lucide;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Home,
    Games,
    Achievements,
    Spoofing,
    TitleSearch,
    Settings,
}

impl Page {
    pub(super) const MAIN: [Self; 5] = [
        Self::Home,
        Self::Games,
        Self::Achievements,
        Self::Spoofing,
        Self::TitleSearch,
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Games => "Games",
            Self::Achievements => "Achievements",
            Self::Spoofing => "Spoofing",
            Self::TitleSearch => "Title search",
            Self::Settings => "Settings",
        }
    }

    pub(super) fn icon(self) -> Lucide {
        match self {
            Self::Home => Lucide::House,
            Self::Games => Lucide::Gamepad2,
            Self::Achievements => Lucide::Trophy,
            Self::Spoofing => Lucide::ClockPlus,
            Self::TitleSearch => Lucide::Search,
            Self::Settings => Lucide::Settings,
        }
    }
}
