//! Contact names and photos for handles (phone numbers / emails), via the
//! macOS Contacts framework. Needs the Contacts permission (one system
//! prompt; the app's Info.plist must carry NSContactsUsageDescription or
//! macOS refuses without asking).

use std::collections::HashMap;
use std::sync::Arc;

use ms_core::types::IngestHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactsAccess {
    Authorized,
    Denied,
    NotDetermined,
    Unsupported,
}

/// One contact as far as Messages Search cares.
#[derive(Debug, Clone)]
pub struct ContactInfo {
    pub name: String,
    /// Thumbnail photo bytes (JPEG/PNG), shared by all of its addresses.
    pub thumbnail: Option<Arc<Vec<u8>>>,
}

/// Matching key for an address: lowercased email, or the last 10 digits of
/// a phone number (so "+1 (555) 555-0101" matches "5555550101").
pub fn key_for(address: &str) -> Option<String> {
    let a = address.trim();
    if a.contains('@') {
        return Some(a.to_lowercase());
    }
    let digits: String = a.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 5 {
        return None;
    }
    let start = digits.len().saturating_sub(10);
    Some(digits[start..].to_string())
}

/// Names for `handles` from an address-key → name map.
pub fn resolve(
    handles: &[IngestHandle],
    names: &HashMap<String, String>,
) -> HashMap<i64, Option<String>> {
    handles
        .iter()
        .map(|h| {
            (
                h.id,
                key_for(&h.address).and_then(|k| names.get(&k).cloned()),
            )
        })
        .collect()
}

/// The contact of each handle that has one (see [`load_contacts`]).
pub fn resolve_contacts<'a>(
    handles: &[IngestHandle],
    contacts: &'a HashMap<String, ContactInfo>,
) -> HashMap<i64, &'a ContactInfo> {
    handles
        .iter()
        .filter_map(|h| Some((h.id, contacts.get(&key_for(&h.address)?)?)))
        .collect()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ptr::NonNull;
    use std::sync::{mpsc, Arc};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::AllocAnyThread;
    use objc2_contacts::{
        CNAuthorizationStatus, CNContact, CNContactEmailAddressesKey, CNContactFamilyNameKey,
        CNContactFetchRequest, CNContactGivenNameKey, CNContactNicknameKey,
        CNContactOrganizationNameKey, CNContactPhoneNumbersKey, CNContactStore,
        CNContactThumbnailImageDataKey, CNEntityType, CNKeyDescriptor,
    };
    use objc2_foundation::{NSArray, NSError, NSString};

    use super::{key_for, ContactInfo, ContactsAccess};

    pub fn access() -> ContactsAccess {
        // SAFETY: plain class method call with a valid enum value.
        let status =
            unsafe { CNContactStore::authorizationStatusForEntityType(CNEntityType::Contacts) };
        match status {
            CNAuthorizationStatus::Authorized | CNAuthorizationStatus::Limited => {
                ContactsAccess::Authorized
            }
            CNAuthorizationStatus::NotDetermined => ContactsAccess::NotDetermined,
            _ => ContactsAccess::Denied,
        }
    }

    /// Show the system prompt (first time only) and wait for the answer.
    /// Must not be called on the main thread.
    pub fn request_access() -> bool {
        let (tx, rx) = mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: Bool, _err: *mut NSError| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: the completion block is retained by the framework until called.
        unsafe {
            let store = CNContactStore::new();
            store.requestAccessForEntityType_completionHandler(CNEntityType::Contacts, &block);
        }
        rx.recv().unwrap_or(false)
    }

    /// Every contact's addresses → name and photo.
    pub fn load_contacts() -> Result<HashMap<String, ContactInfo>, String> {
        let contacts: RefCell<HashMap<String, ContactInfo>> = RefCell::new(HashMap::new());
        // SAFETY: the keys are framework constants; the enumeration block only
        // reads properties that were requested in `keys`.
        unsafe {
            let store = CNContactStore::new();
            let keys: [&NSString; 7] = [
                CNContactGivenNameKey,
                CNContactFamilyNameKey,
                CNContactNicknameKey,
                CNContactOrganizationNameKey,
                CNContactPhoneNumbersKey,
                CNContactEmailAddressesKey,
                CNContactThumbnailImageDataKey,
            ];
            let descriptors: Vec<&ProtocolObject<dyn CNKeyDescriptor>> =
                keys.iter().map(|k| ProtocolObject::from_ref(*k)).collect();
            let keys = NSArray::from_slice(&descriptors);
            let request =
                CNContactFetchRequest::initWithKeysToFetch(CNContactFetchRequest::alloc(), &keys);
            let block = RcBlock::new(|contact: NonNull<CNContact>, _stop: NonNull<Bool>| {
                let c = contact.as_ref();
                let given = c.givenName().to_string();
                let family = c.familyName().to_string();
                let full = format!("{given} {family}").trim().to_string();
                let name = [
                    full,
                    c.nickname().to_string(),
                    c.organizationName().to_string(),
                ]
                .into_iter()
                .find(|s| !s.trim().is_empty());
                let Some(name) = name else { return };
                let thumbnail = c
                    .thumbnailImageData()
                    .map(|d| d.to_vec())
                    .filter(|d| !d.is_empty())
                    .map(Arc::new);
                let info = ContactInfo { name, thumbnail };
                let mut map = contacts.borrow_mut();
                for lv in c.phoneNumbers().iter() {
                    if let Some(k) = key_for(&lv.value().stringValue().to_string()) {
                        map.entry(k).or_insert_with(|| info.clone());
                    }
                }
                for lv in c.emailAddresses().iter() {
                    let email: Retained<NSString> = lv.value();
                    if let Some(k) = key_for(&email.to_string()) {
                        map.entry(k).or_insert_with(|| info.clone());
                    }
                }
            });
            let mut err: Option<Retained<NSError>> = None;
            let ok = store.enumerateContactsWithFetchRequest_error_usingBlock(
                &request,
                Some(&mut err),
                &block,
            );
            if !ok {
                return Err(err.map_or_else(
                    || "contacts enumeration failed".to_string(),
                    |e| e.localizedDescription().to_string(),
                ));
            }
        }
        Ok(contacts.into_inner())
    }
}

#[cfg(target_os = "macos")]
pub use imp::{access, load_contacts, request_access};

#[cfg(not(target_os = "macos"))]
pub fn access() -> ContactsAccess {
    ContactsAccess::Unsupported
}
#[cfg(not(target_os = "macos"))]
pub fn request_access() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn load_contacts() -> Result<HashMap<String, ContactInfo>, String> {
    Ok(HashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(key_for("+1 (555) 555-0101").as_deref(), Some("5555550101"));
        assert_eq!(key_for("5555550101").as_deref(), Some("5555550101"));
        assert_eq!(
            key_for("Mike@Ross.example").as_deref(),
            Some("mike@ross.example")
        );
        assert_eq!(key_for("1234"), None);
    }
}
