use rand::distr::{Alphanumeric, SampleString};

#[derive(Debug)]
pub struct SubscriptionToken(String);

impl SubscriptionToken {
    const TOKEN_LENGTH: usize = 25;

    pub fn new() -> Self {
        Self(Alphanumeric.sample_string(&mut rand::rng(), 25))
    }

    pub fn parse(s: String) -> Result<Self, String> {
        let is_wrong_length = s.len() != Self::TOKEN_LENGTH;
        let is_not_alphanumeric = !s.chars().all(|c| c.is_ascii_alphanumeric());

        if is_wrong_length || is_not_alphanumeric {
            Err(format!(
                "Token must be {} alphanumeric characters.",
                Self::TOKEN_LENGTH
            ))
        } else {
            Ok(Self(s))
        }
    }
}

impl AsRef<str> for SubscriptionToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Default for SubscriptionToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::SubscriptionToken;
    use claims::assert_err;

    #[quickcheck_macros::quickcheck]
    fn new_tokens_are_accepted() -> bool {
        let token = SubscriptionToken::new();
        SubscriptionToken::parse(token.as_ref().to_owned()).is_ok()
    }

    #[quickcheck_macros::quickcheck]
    fn random_strings_are_usually_rejected(s: String) -> bool {
        if s.len() == SubscriptionToken::TOKEN_LENGTH
            && s.chars().all(|c| c.is_ascii_alphanumeric())
        {
            SubscriptionToken::parse(s).is_ok()
        } else {
            SubscriptionToken::parse(s).is_err()
        }
    }

    #[test]
    fn empty_string_is_rejected() {
        let token = "".to_string();
        assert_err!(SubscriptionToken::parse(token));
    }

    #[test]
    fn string_1_char_longer_than_token_length_is_rejected() {
        let token = "a".repeat(SubscriptionToken::TOKEN_LENGTH + 1);
        assert_err!(SubscriptionToken::parse(token));
    }

    #[test]
    fn string_1_char_shorter_than_token_length_is_rejected() {
        let token = "a".repeat(SubscriptionToken::TOKEN_LENGTH - 1);
        assert_err!(SubscriptionToken::parse(token));
    }

    #[test]
    fn correct_length_with_special_characters_is_rejected() {
        let mut token = "a".repeat(SubscriptionToken::TOKEN_LENGTH - 1);
        token.push('!');
        assert_err!(SubscriptionToken::parse(token));
    }

    #[test]
    fn correct_length_with_whitespace_is_rejected() {
        let mut token = "a".repeat(SubscriptionToken::TOKEN_LENGTH - 1);
        token.push(' ');
        assert_err!(SubscriptionToken::parse(token));
    }
}
