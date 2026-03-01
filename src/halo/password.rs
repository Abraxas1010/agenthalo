pub const MIN_PASSWORD_LEN: usize = 8;
pub const RECOMMENDED_PASSWORD_LEN: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordStrength {
    TooWeak,
    Weak,
    Moderate,
    Strong,
    VeryStrong,
}

impl PasswordStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TooWeak => "too-weak",
            Self::Weak => "weak",
            Self::Moderate => "moderate",
            Self::Strong => "strong",
            Self::VeryStrong => "very-strong",
        }
    }
}

pub fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(format!(
            "password must be at least {} characters",
            MIN_PASSWORD_LEN
        ));
    }
    if password.trim().is_empty() {
        return Err("password must not be blank".to_string());
    }
    if password.bytes().any(|b| b == 0) {
        return Err("password must not contain null bytes".to_string());
    }
    if matches!(
        estimate_strength(password),
        PasswordStrength::TooWeak | PasswordStrength::Weak
    ) {
        return Err("password is too weak".to_string());
    }
    Ok(())
}

pub fn validate_password_pair(password: &str, confirm: &str) -> Result<(), String> {
    validate_password(password)?;
    if password != confirm {
        return Err("password and confirmation do not match".to_string());
    }
    Ok(())
}

pub fn estimate_strength(password: &str) -> PasswordStrength {
    if password.len() < MIN_PASSWORD_LEN {
        return PasswordStrength::TooWeak;
    }
    let classes = char_classes(password);
    if password.len() >= 16 && classes >= 4 {
        return PasswordStrength::VeryStrong;
    }
    if password.len() >= 12 && classes >= 3 {
        return PasswordStrength::Strong;
    }
    if password.len() >= RECOMMENDED_PASSWORD_LEN || classes >= 3 {
        return PasswordStrength::Moderate;
    }
    PasswordStrength::Weak
}

fn char_classes(password: &str) -> usize {
    let mut lower = false;
    let mut upper = false;
    let mut digit = false;
    let mut symbol = false;
    for ch in password.chars() {
        if ch.is_ascii_lowercase() {
            lower = true;
        } else if ch.is_ascii_uppercase() {
            upper = true;
        } else if ch.is_ascii_digit() {
            digit = true;
        } else if !ch.is_whitespace() {
            symbol = true;
        }
    }
    [lower, upper, digit, symbol]
        .into_iter()
        .filter(|x| *x)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_passwords() {
        assert!(validate_password("short").is_err());
    }

    #[test]
    fn pair_validation_works() {
        assert!(validate_password_pair("LongerPass123!", "LongerPass123!").is_ok());
        assert!(validate_password_pair("LongerPass123!", "LongerPass124!").is_err());
    }

    #[test]
    fn strength_ordering_is_reasonable() {
        assert_eq!(estimate_strength("abcd"), PasswordStrength::TooWeak);
        assert_eq!(estimate_strength("abcdefgh"), PasswordStrength::Weak);
        assert_eq!(estimate_strength("Abcdefgh12"), PasswordStrength::Moderate);
        assert_eq!(estimate_strength("Abcdefgh1234!"), PasswordStrength::Strong);
        assert_eq!(
            estimate_strength("Abcdefgh1234!@#$"),
            PasswordStrength::VeryStrong
        );
    }
}
