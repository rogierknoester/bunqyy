use openssl::base64;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::sign::Signer as OpenSSLSigner;
use tracing::debug;

/// Generate a new keypair that can be used with bunqyy's api.
/// bunqyy requires the use of rsa with 2048 bits.
///
/// ```
/// let keypair = generate_key();
/// ```
///
/// will panic if it cannot generate a keypair
pub(crate) fn generate_keypair() -> PKey<Private> {
    debug!("Generating new RSA keypair");

    let rsa = Rsa::generate(2048).expect("Cannot generate rsa");
    let private = PKey::from_rsa(rsa);

    private.expect("Cannot generate private key")
}

/// Sign the passed data with the provided private key
/// will return the signed data as a base64 encoded string
fn sign_bytes_data_to_string(data: &[u8], private_key_pem: String) -> String {
    let private_key = PKey::private_key_from_pem(private_key_pem.as_bytes()).unwrap();

    let mut signer = OpenSSLSigner::new(MessageDigest::sha256(), &private_key).unwrap();
    signer.update(data).expect("Cannot sign data");

    let signature = signer.sign_to_vec().unwrap();

    let as_base64 = base64::encode_block(signature.as_ref());

    as_base64
}

pub type Signer = Box<dyn FnOnce(&[u8]) -> String + Send>;

/// Create a one-time use signer
/// ```
/// let signer = create_signer(keypair.private_key_to_pem_pkcs8());
/// let signed_data = signer("my-payload-string".as_bytes());
pub(crate) fn create_signer(private_key_pem: String) -> Signer {
    Box::new(|data| sign_bytes_data_to_string(data, private_key_pem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_bytes_data_to_string() {
        let keypair = generate_keypair();
        let private_key_pem =
            String::from_utf8_lossy(keypair.private_key_to_pem_pkcs8().unwrap().as_ref())
                .to_string();

        let data = "my-payload-string".as_bytes();

        let signed_data = sign_bytes_data_to_string(data, private_key_pem);

        assert_eq!(signed_data.len(), 344);
    }

    #[test]
    fn test_create_signer() {
        let keypair = generate_keypair();
        let private_key_pem =
            String::from_utf8_lossy(keypair.private_key_to_pem_pkcs8().unwrap().as_ref())
                .to_string();

        let signer = create_signer(private_key_pem);

        let data = "my-payload-string".as_bytes();

        let signed_data = signer(data);

        assert_eq!(signed_data.len(), 344);
    }
}
