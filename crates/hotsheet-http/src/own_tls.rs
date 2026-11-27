//! TLS with a caller's own rustls configuration (HS2-9ABVDW). ureq 3 builds its rustls
//! `ClientConfig` itself from a `TlsConfig`, which has no room for an ALPN list or a
//! pinned provider choice per agent. Callers with their own trust (a pinned CA, a client
//! certificate) already build a complete `ClientConfig`; this connector uses it exactly
//! as given, so their TLS settings stay what they were on ureq 2.

use std::io::{Read, Write};
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use ureq::Error;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout, Transport,
    TransportAdapter,
};

/// Wraps an `https` connection in TLS with the given configuration; leaves `http` alone.
#[derive(Debug)]
pub(crate) struct OwnTlsConnector(pub(crate) Arc<ClientConfig>);

impl<In: Transport> Connector<In> for OwnTlsConnector {
    type Out = Either<In, OwnTlsTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        let Some(transport) = chained else {
            return Ok(None);
        };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }
        let host = details
            .uri
            .host()
            .ok_or_else(|| Error::BadUri(details.uri.to_string()))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let name = ServerName::try_from(host.to_owned())
            .map_err(|_| Error::Tls("invalid server name for TLS"))?;
        let mut connection = ClientConnection::new(self.0.clone(), name)?;
        let mut socket = TransportAdapter::new(transport.boxed(), details.current_time.clone());
        socket.set_timeout(details.timeout);
        connection.complete_io(&mut socket)?;
        Ok(Some(Either::B(OwnTlsTransport {
            buffers: LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            ),
            stream: StreamOwned::new(connection, socket),
        })))
    }
}

/// An established TLS connection over the chained transport.
pub(crate) struct OwnTlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl std::fmt::Debug for OwnTlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnTlsTransport").finish_non_exhaustive()
    }
}

impl Transport for OwnTlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}
