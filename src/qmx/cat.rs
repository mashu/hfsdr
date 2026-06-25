//! Kenwood-style CAT control for QRP Labs QMX / QMX+ transceivers.
//!
//! Commands are semicolon-terminated with no CR/LF. See the QMX CAT manual.

use std::io::{Read, Write};
use std::time::Duration;

use serialport::SerialPort;

use crate::source::{Result, SourceError};

const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// USB Virtual COM port to the QMX CAT interface.
pub struct CatPort {
    #[cfg(any(test, coverage, mock_hal))]
    mock: Option<crate::mock_hal::MockCat>,
    #[cfg(any(test, coverage, mock_hal))]
    port: Option<Box<dyn SerialPort>>,
    #[cfg(not(any(test, coverage, mock_hal)))]
    port: Box<dyn SerialPort>,
}

impl CatPort {
    pub fn open(path: &str) -> Result<Self> {
        #[cfg(any(test, coverage, mock_hal))]
        if crate::mock_hal::enabled() {
            let _ = path;
            return Ok(Self {
                mock: Some(crate::mock_hal::MockCat::default()),
                port: None,
            });
        }
        let port = serialport::new(path, 9_600)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|e| SourceError::Unsupported(format!("open serial {path}: {e}")))?;
        #[cfg(any(test, coverage, mock_hal))]
        let mut cat = Self {
            mock: None,
            port: Some(port),
        };
        #[cfg(not(any(test, coverage, mock_hal)))]
        let mut cat = Self { port };
        cat.set_timeout(READ_TIMEOUT)?;
        Ok(cat)
    }

    #[cfg(any(test, coverage, mock_hal))]
    fn mock_mut(&mut self) -> Option<&mut crate::mock_hal::MockCat> {
        self.mock.as_mut()
    }

    #[cfg(any(test, coverage, mock_hal))]
    fn port_mut(&mut self) -> Result<&mut Box<dyn SerialPort>> {
        self.port
            .as_mut()
            .ok_or(SourceError::InvalidState("mock cat has no serial port"))
    }

    fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if self.mock_mut().is_some() {
            return Ok(());
        }
        #[cfg(any(test, coverage, mock_hal))]
        return self.port_mut()?.set_timeout(timeout).map_err(|e| SourceError::Unsupported(e.to_string()));
        #[cfg(not(any(test, coverage, mock_hal)))]
        self.port.set_timeout(timeout).map_err(|e| SourceError::Unsupported(e.to_string()))
    }

    /// Send one or more CAT commands (each must include the trailing `;`).
    pub fn send(&mut self, cmd: &str) -> Result<()> {
        if cmd.contains('\r') || cmd.contains('\n') {
            return Err(SourceError::Unsupported(
                "CAT commands must not contain CR/LF".into(),
            ));
        }
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.send(cmd);
        }
        #[cfg(any(test, coverage, mock_hal))]
        let port = self.port_mut()?;
        #[cfg(not(any(test, coverage, mock_hal)))]
        let port = &mut self.port;
        port
            .write_all(cmd.as_bytes())
            .map_err(|e| SourceError::Backend {
                op: "cat_write",
                code: e.raw_os_error().unwrap_or(-1),
            })?;
        port.flush().map_err(|e| SourceError::Backend {
            op: "cat_flush",
            code: e.raw_os_error().unwrap_or(-1),
        })?;
        Ok(())
    }

    /// Send a query command and read until `;` terminates the response.
    pub fn query(&mut self, cmd: &str) -> Result<String> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.query(cmd);
        }
        self.send(cmd)?;
        let mut buf = Vec::with_capacity(64);
        let mut chunk = [0u8; 64];
        loop {
            #[cfg(any(test, coverage, mock_hal))]
            let port = self.port_mut()?;
            #[cfg(not(any(test, coverage, mock_hal)))]
            let port = &mut self.port;
            let n = port.read(&mut chunk).map_err(|e| SourceError::Backend {
                op: "cat_read",
                code: e.raw_os_error().unwrap_or(-1),
            })?;
            if n == 0 {
                return Err(SourceError::Unsupported("CAT read timeout".into()));
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.iter().any(|&b| b == b';') {
                break;
            }
            if buf.len() > 256 {
                return Err(SourceError::Unsupported("CAT response too long".into()));
            }
        }
        String::from_utf8(buf).map_err(|e| SourceError::Unsupported(e.to_string()))
    }

    pub fn set_iq_mode(&mut self, on: bool) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.set_iq_mode(on);
        }
        self.send(&format!("Q9{};", u8::from(on)))
    }

    pub fn ensure_receive(&mut self) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.ensure_receive();
        }
        self.send("RX;")
    }

    pub fn set_cat_timeout_enabled(&mut self, on: bool) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.set_cat_timeout_enabled(on);
        }
        self.send(&format!("QB{};", u8::from(on)))
    }

    /// Set VFO A frequency in Hz (11-digit Kenwood format).
    pub fn set_vfo_a_hz(&mut self, hz: u64) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.set_vfo_a_hz(hz);
        }
        self.send(&format!("FA{hz:011};"))
    }

    pub fn set_rf_gain_db(&mut self, db: u8) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.set_rf_gain_db(db);
        }
        self.send(&format!("RG{db:03};"))
    }

    pub fn set_operating_mode_cw(&mut self) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.set_operating_mode_cw();
        }
        self.send("MD3;")
    }

    pub fn is_transmitting(&mut self) -> Result<bool> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.is_transmitting();
        }
        let resp = self.query("TQ;")?;
        Ok(resp.contains("TQ1"))
    }

    pub fn read_smeter_db(&mut self) -> Result<Option<f32>> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            return m.read_smeter_db();
        }
        let resp = self.query("SM;")?;
        parse_smeter_db(&resp)
    }
}

/// List available serial ports (for UI device pickers).
pub fn list_serial_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

fn parse_smeter_db(resp: &str) -> Result<Option<f32>> {
    let digits: String = resp.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Ok(None);
    }
    digits
        .parse::<f32>()
        .map(Some)
        .map_err(|e| SourceError::Unsupported(format!("SM parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_smeter() {
        assert_eq!(parse_smeter_db("SM045;").unwrap(), Some(45.0));
        assert_eq!(parse_smeter_db("SM;").unwrap(), None);
        assert_eq!(parse_smeter_db("SM+12;").unwrap(), Some(12.0));
        assert_eq!(parse_smeter_db("SMabc;").unwrap(), None);
    }

    #[test]
    fn parse_smeter_ignores_non_digit_noise() {
        assert_eq!(parse_smeter_db("SM+12dB;").unwrap(), Some(12.0));
        assert_eq!(parse_smeter_db("SM.;").unwrap(), None);
    }
}
