# Changelog
All notable changes to Mles project will be documented in this file after 1.0-release.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
