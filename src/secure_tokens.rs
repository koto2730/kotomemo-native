use std::collections::HashMap;
use std::sync::OnceLock;
use keyring::Entry;

const SERVICE_NAME: &str = "com.ictglabo.kotomemo";
const TEST_SERVICE_NAME: &str = "com.ictglabo.kotomemo-test";

static KEYRING_AVAILABLE: OnceLock<bool> = OnceLock::new();

fn is_keyring_available() -> bool {
    *KEYRING_AVAILABLE.get_or_init(|| {
        let entry = Entry::new(TEST_SERVICE_NAME, "test_probe");
        match entry {
            Ok(e) => {
                if e.set_password("probe_val").is_err() {
                    return false;
                }
                if e.get_password().is_err() {
                    return false;
                }
                e.delete_credential().is_ok()
            }
            Err(_) => false,
        }
    })
}

pub fn set_token(key: &str, value: &str) -> bool {
    if !is_keyring_available() {
        return false;
    }
    if let Ok(entry) = Entry::new(SERVICE_NAME, key) {
        entry.set_password(value).is_ok()
    } else {
        false
    }
}

pub fn get_token(key: &str) -> Option<String> {
    if !is_keyring_available() {
        return None;
    }
    if let Ok(entry) = Entry::new(SERVICE_NAME, key) {
        entry.get_password().ok()
    } else {
        None
    }
}

pub fn delete_token(key: &str) {
    if !is_keyring_available() {
        return;
    }
    if let Ok(entry) = Entry::new(SERVICE_NAME, key) {
        let _ = entry.delete_credential();
    }
}

pub fn sync_tokens_on_load(config_tokens: HashMap<String, String>) -> HashMap<String, String> {
    if !is_keyring_available() {
        return config_tokens;
    }

    let mut result = HashMap::new();
    for (key, val) in config_tokens {
        if !val.is_empty() {
            set_token(&key, &val);
            result.insert(key, val);
        } else {
            let loaded = get_token(&key).unwrap_or_default();
            result.insert(key, loaded);
        }
    }
    result
}

pub fn prepare_tokens_for_save(tokens: &HashMap<String, String>) -> HashMap<String, String> {
    if !is_keyring_available() {
        return tokens.clone();
    }

    let mut disk_tokens = HashMap::new();
    for (key, val) in tokens {
        set_token(key, val);
        disk_tokens.insert(key.clone(), String::new());
    }
    disk_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow() {
        if !is_keyring_available() {
            println!("Keyring not available on this platform/environment, skipping test.");
            return;
        }

        let key = "test_key_abc";
        let val = "super_secret_123";

        assert!(set_token(key, val));
        assert_eq!(get_token(key), Some(val.to_string()));

        delete_token(key);
        assert_eq!(get_token(key), None);
    }
}
