pub mod account {
    use console::{style, StyledObject};

    pub fn me(s: impl ToString) -> StyledObject<String> {
        style(s.to_string()).cyan().bold().bright()
    }

    pub fn other(s: impl ToString) -> StyledObject<String> {
        style(s.to_string()).magenta().bold().bright()
    }
}

pub mod text {
    use console::{style, StyledObject};

    pub fn dim(s: impl ToString) -> StyledObject<String> {
        style(s.to_string()).black().bright()
    }

    pub fn error(s: impl ToString) -> StyledObject<String> {
        style(s.to_string()).red()
    }

    pub fn control(s: impl ToString) -> StyledObject<String> {
        style(s.to_string()).green()
    }

    pub fn command(s: impl ToString) -> StyledObject<String> {
        style(s.to_string()).green().bold()
    }
}
