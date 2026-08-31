/*
 * Helpers base64 usados por el streaming de archivos via SSH.
 * Extraido de ssh_client.rs (Fase H): helpers puros y autocontenidos
 * (sin estado) para mantener el cliente SSH bajo el limite de lineas.
 */

use crate::error::CoolifyError;

pub fn base64_encode(data: &[u8]) -> String {
    /* Implementacion simple con chunks para evitar problemas de longitud de linea */
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let combined = (b0 << 16) | (b1 << 8) | b2;
        result.push(chars[((combined >> 18) & 0x3F) as usize] as char);
        result.push(chars[((combined >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(chars[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(chars[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

pub fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, CoolifyError> {
    let input = input.replace(['\n', '\r', ' '], "");
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();

    let lookup = |c: u8| -> std::result::Result<u32, CoolifyError> {
        if c == b'=' {
            return Ok(0);
        }
        chars
            .iter()
            .position(|&ch| ch == c)
            .map(|p| p as u32)
            .ok_or_else(|| {
                CoolifyError::Validation(format!("Caracter base64 invalido: {}", c as char))
            })
    };

    for chunk in input.as_bytes().chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let b0 = lookup(chunk[0])?;
        let b1 = lookup(chunk[1])?;
        let b2 = lookup(chunk[2])?;
        let b3 = lookup(chunk[3])?;
        let combined = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        result.push(((combined >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            result.push(((combined >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            result.push((combined & 0xFF) as u8);
        }
    }

    Ok(result)
}
