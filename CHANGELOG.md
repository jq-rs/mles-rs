# Changelog
All notable changes to Mles project will be documented in this file after 1.0-release.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [2.0.0]

Upgraded to Mles v2.

## [1.1.6]

Updated to support IPv6 by default.

## [1.1.5]

Downgrade bytes back to 0.4.

## [1.1.4]

Clippy warning and test fixes.

## [1.1.3]

Update to bytes 0.5, siphasher 0.3 and serde_bytes 0.11.

## [1.1.2]

Specification:
 - Include TLS support as an option for session transport 

Client:
 - Change WebSocket proxy to use a task instead of thread for every connection

Utils:
 - Add mutable message get

Sysutils:
 - Fix paths, define longer history by default

## [1.1.1]

Tokio upgrade from core to current Tokio.

## [1.1.0]

Generic:
 - Defined specifications for multiplexing channels over one WebSocket connection.
 - Added existing Mles protocol and Mles Websocket client implementations to the README
 - Updated to support Rust 2018
 - Updated some crates

Utils 1.1.0:
 - Use serde cbor 0.9

Client 1.1.0:
 - Use tungstenite 0.6 and tokio-tungstenite 0.6

## [1.0.6]

### Added

Generic:
 - Updated README to include IP address format details.

### Changed

Utils 1.0.6:
 - Use serde cbor 0.8
 - Update to use BytesMut::unsplit()

Client 1.0.6:
 - Change encoder to use extend_from_slice()

## [1.0.5]

### Added

Generic:
 - README Information about mles-websocket IANA registration

### Fixed

Client 1.0.5:
 - Encoder buffer handling fixed. This could cause websocket connection losses with large traffic amount.

## [1.0.4]

### Fixed

Utils 1.0.4:
 - Header read len mask fix. Allows to use larger than 4k frames.
 - Fix stream write to use write_all(). 

## [1.0.3]

### Changed

Utils 1.0.3:
 - Bytes crate taken into use. Message forwarding performance should improve significantly.

## [1.0.2]

### Added

Generic:
 - README environment variable example added.

### Fixed

Generic:
 - README fixes.

Client 1.0.2:
 - Removed WebSocket-proxy message receive mirroring. Now messages are not mirrored back to WebSocket client unnecessarily.

## [1.0.1]

### Changed

 Generic:
 - README fixes
 - README ResyncMsg clarification
 - README Mles WebSocket proxy protocol specification
 - Systemd-scripts moved to sysutils-directory
 
 Utils 1.0.1:
 - Documentation typo fixes
 
 Client 1.0.1:
   - Proxy Websocket library version update
   - Support for proper handling of Sec-WebSocket-Protocol 

## [1.0.0]

### Added

Basic Mles-protocol support on Mles utils-library with server and client.
 * All relevant Mles-protocol functionalities implemented which include
   - Authentication with key and/or address
   - Connection id handling for loop freedom
   - Resynchronization support where Mles peers are able to offer resiliency 
     functionality to Mles root server
