use uuid::Uuid;
use vault_core::{PUBLIC_KEY_LEN, PublicKey, SyncState};

use super::*;
use crate::noise::{PairingSecret, SECRET_LEN, new_identity};

fn invitation() -> Invitation {
  Invitation {
    secret: PairingSecret::from_bytes(std::array::from_fn(|i| u8::try_from(i).unwrap())),
    inviter: PublicKey([0xee; PUBLIC_KEY_LEN]),
  }
}

fn state_with_identity() -> SyncState {
  SyncState {
    identity: Some(new_identity(Uuid::from_u128(1), "Computer".to_string()).unwrap()),
    ..SyncState::default()
  }
}

// ---- QR code ----------------------------------------------------------------

#[test]
fn the_qr_text_round_trips() {
  let text = invitation().qr_text();

  assert!(text.starts_with("ARSU-INVITE:"));
  assert_eq!(Invitation::from_qr_text(&text).unwrap(), invitation());
}

#[test]
fn the_qr_text_is_uppercase_alphanumeric_for_a_compact_qr_code() {
  let text = invitation().qr_text();

  assert!(
    text
      .chars()
      .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == ':')
  );
}

#[test]
fn other_qr_codes_are_not_invitations() {
  for text in [
    "otpauth://totp/x?secret=ABC",
    "ARSU-INVITE:",
    "ARSU-INVITE:0000",
    "ARSU-INVITE:!!!",
  ] {
    assert_eq!(
      Invitation::from_qr_text(text),
      Err(InvitationError::NotAnInvitation),
      "{text}"
    );
  }
}

#[test]
fn a_newer_qr_format_is_reported_as_such() {
  let mut bytes = vec![2u8];
  bytes.extend([0u8; SECRET_LEN + PUBLIC_KEY_LEN]);
  let text = format!(
    "ARSU-INVITE:{}",
    base32::encode(base32::Alphabet::Crockford, &bytes)
  );

  assert_eq!(
    Invitation::from_qr_text(&text),
    Err(InvitationError::UnsupportedVersion)
  );
}

// ---- text code --------------------------------------------------------------

#[test]
fn the_code_is_eleven_groups_of_five() {
  let code = invitation().code();
  let groups: Vec<&str> = code.split('-').collect();

  assert_eq!(groups.len(), 11);
  assert!(groups.iter().all(|group| group.len() == 5));
}

#[test]
fn the_code_round_trips_to_the_secret() {
  assert_eq!(
    parse_code(&invitation().code()).unwrap(),
    invitation().secret
  );
}

#[test]
fn the_code_is_read_whatever_the_case_spacing_and_lookalike_letters() {
  let typed = invitation()
    .code()
    .to_lowercase()
    .replace('-', " ")
    .replace('0', "o")
    .replace('1', "l");

  assert_eq!(parse_code(&typed).unwrap(), invitation().secret);
}

#[test]
fn a_mistyped_code_is_caught() {
  let code = invitation().code();
  let mut typo: Vec<char> = code.chars().collect();
  typo[3] = if typo[3] == 'A' { 'B' } else { 'A' };
  let typo: String = typo.into_iter().collect();

  assert_eq!(parse_code(&typo), Err(InvitationError::BadChecksum));
}

#[test]
fn a_code_of_the_wrong_length_is_not_a_code() {
  assert_eq!(
    parse_code("ABCDE-FGHJK"),
    Err(InvitationError::NotAnInvitation)
  );
  assert_eq!(parse_code(""), Err(InvitationError::NotAnInvitation));
  assert_eq!(
    parse_code("not a code at all!"),
    Err(InvitationError::NotAnInvitation)
  );
}

// ---- either form ------------------------------------------------------------

#[test]
fn a_qr_text_reads_as_its_secret_and_inviter() {
  let read = read(&invitation().qr_text()).unwrap();

  assert_eq!(read, (invitation().secret, Some(invitation().inviter)));
}

#[test]
fn a_typed_code_reads_as_its_secret_alone() {
  let read = read(&format!("  {}  ", invitation().code().as_str())).unwrap();

  assert_eq!(read, (invitation().secret, None));
}

#[test]
fn anything_else_is_not_an_invitation() {
  assert_eq!(read("hello"), Err(InvitationError::NotAnInvitation));
}

// ---- advertisement ----------------------------------------------------------

#[test]
fn the_advertisement_token_depends_only_on_the_secret() {
  let a = PairingSecret::from_bytes([1; SECRET_LEN]);
  let b = PairingSecret::from_bytes([2; SECRET_LEN]);

  assert_eq!(advertisement_token(&a), advertisement_token(&a.clone()));
  assert_ne!(advertisement_token(&a), advertisement_token(&b));
  assert!(!a.expose().starts_with(&advertisement_token(&a)));
}

// ---- pending invitations ----------------------------------------------------

#[test]
fn an_opened_invitation_carries_this_devices_key_and_is_pending() {
  let mut state = state_with_identity();

  let invitation = open(&mut state, 1_000).unwrap();

  assert_eq!(
    Some(invitation.inviter),
    state.identity.as_ref().map(|identity| identity.public_key)
  );
  assert_eq!(state.invitations.len(), 1);
  assert_eq!(state.invitations[0].expires_at, 1_000 + LIFETIME);
  assert_eq!(open_secrets(&state, 1_000), vec![invitation.secret]);
}

#[test]
fn an_invitation_cannot_be_opened_before_sync_is_set_up() {
  assert_eq!(
    open(&mut SyncState::default(), 1),
    Err(InvitationError::NoIdentity)
  );
}

#[test]
fn an_invitation_expires() {
  let mut state = state_with_identity();
  open(&mut state, 1_000).unwrap();

  assert_eq!(
    open_secrets(&state, 1_000 + LIFETIME).len(),
    1,
    "last second"
  );
  assert!(open_secrets(&state, 1_001 + LIFETIME).is_empty());

  open(&mut state, 2_000 + LIFETIME).unwrap();
  assert_eq!(state.invitations.len(), 1, "the expired one was dropped");
}

#[test]
fn an_invitation_is_used_once() {
  let mut state = state_with_identity();
  let invitation = open(&mut state, 1).unwrap();

  assert!(consume(&mut state, &invitation.secret, 1));
  assert!(!consume(&mut state, &invitation.secret, 1));
  assert!(open_secrets(&state, 1).is_empty());
}

#[test]
fn an_expired_invitation_cannot_be_used() {
  let mut state = state_with_identity();
  let invitation = open(&mut state, 1_000).unwrap();

  assert!(!consume(&mut state, &invitation.secret, 1_001 + LIFETIME));
  assert!(state.invitations.is_empty(), "it is dropped all the same");
}

#[test]
fn only_the_newest_invitations_stay_open() {
  let mut state = state_with_identity();
  let first = open(&mut state, 1).unwrap();
  for _ in 0..MAX_OPEN {
    open(&mut state, 1).unwrap();
  }

  assert_eq!(state.invitations.len(), MAX_OPEN);
  assert!(!open_secrets(&state, 1).contains(&first.secret));
}
