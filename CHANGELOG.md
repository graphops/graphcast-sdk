# Changelog

All notable changes to this project will be documented in this file.

## [unreleased]

### Bug Fixes

- Fix peer count issue
- Waku node setup with filter protocol only
- IndexingStatuses graphQL schema allow null node
- Add fantom network name
- Ensure content topic lock gets released
- Disable unsubscribe until waku dep update
- Callbook input order
- DNS and Discv5 peer connection
- Allow indexer without operator
- Consistent nonce during build
- Repo link in unrecognised chain message (#265)
- Graph account owned current version hash query
- Discv5 toggle
- Boot node connections
- Empty set_waku_event_callback at shutdown, refactor mpsc sender
- Remove filter subscription from content topic update

### Documentation

- Fix a grammatical error on the readme
- Updated pull request template (#153)
- New release process and changelog script
- Update release process and script

### Features

- Support and refactor for query radio (#125)
- Clap CLI args (#130)
- Basic args validation and improve logs (#133)
- Add Discord bot support
- Mnemonics allowed, refactor network config (#140)
- Auto approve and merge pr by dependabot (#149)
- Topic coverage toggle, indexingStatuses (#150)
- Add prometheus toggle
- Add metrics_host to config
- Graphql for messages and add server host and port
- Add options to toggle logger format
- Add telegram notifications
- Enable discv5 configs
- Graphcast agent relay/filter protocol toggle
- Validation mechanism options for msg sender identity
- Subgraph owner, id, and deployment hash check
- Chainable msg type decoding and handling
- Add waku error type string
- Replace Polygon with Matic
- Peer node data helper fns
- Limit graceful shutdown interval and add force exit
- Add content topic check at message handle
- PeerData GraphQL type, refactor peer helpers
- Add RadioPayload trait alias and requirements
- Allow content topic matching when topic list is empty

### Miscellaneous Tasks

- V0.0.14 (#137)
- Bump version
- 0.0.17
- Remove dependabot auto-merge
- Add e2e test helpers
- Release v0.1.1
- Release vv0.1.2
- Release v0.2.0
- Update deps, use waku with seen cache
- Switch to git dep for Waku
- Release 0.3.1
- Release 0.3.2
- Release v0.3.3
- Release 0.3.4
- Fix waku version
- Release 0.4.0
- Add labels workflow
- Update labels workflow
- Release 0.4.1
- Release 0.4.2
- Release 0.4.2
- Release 0.4.3
- Remove labels workflow
- Update waku rust binding for sni fix
- Update bindings version to use crates.io
- Release 0.5.0
- Waku version 0.3.1 for peer count fix
- Release 0.5.0
- Release 0.5.1
- Release 0.5.2
- Release 0.6.0
- Release 0.6.1
- Update waku bindings version
- 0.7.0

### Refactor

- Replace bool prometheus_metrics with u16 metrics_port config var
- Add new error variant to BuildMessage
- Parse grt units, update log levels
- Move config parsing to Radio
- Update ping-pong Radio
- Update query and add configurations
- Change radio_name to be String
- Callbook and graphcast_id struct, clean query fns
- Remove some unnecessary clones
- String -> &str
- Graphcast message requires payload
- Less msg fields required
- Verification fn moved to Account
- Optional graph node endpoint
- Gc validity not automatically checked
- Generic message validation
- Add IdValidations to_string
- Resolve for latest deployment hash instead of a vec
- Move msg channel and re-export waku msg type
- Refactor message decoding
- Move main loop to dedicated function
- Agent signal handler and waku msg receiver repositioned
- Default to relay protocol - relay_publish messages
- Decode function and update example
- Change nonce type
- Add discovery enr back to SDK
- Slack bot switched to using webhook instead of bot token

## [0.0.13] - 2023-03-07

### Bug Fixes

- Rollback bindings dep
- Accurate logs, utilize cf namesever, exposed DNS endpoint (#56)
- Dependabot commit messages (#68)
- Explicit address check (#111)

### Feat

- Block hash from indexing statuses query (#97)

### Features

- Initial slack bot messaging (#24)
- Consistent DNS discovery url (#34)
- Add ping-pong Radio
- Allow users to configure subgraph urls and subtopics
- Dynamic radio payload def (#54)
- Waku_node_key and waku_log_level (#78)
- Bump waku binding with filter and relay topic (#80)
- Configurable pubsub topic (#96)
- Resolve dns url (#107)
- Automated release script (#119)

### Fix

- Additional sender id check (#113)

### Miscellaneous Tasks

- Add husky hook + dependabot (#65)
- Publish to crates.io
- Bump version
- Bump waku bindings
- Publish to crates.io

### Performance

- Protocol in pubsub topic + handle null blocks (#59)
- Periodic network check + automatic reconnect (#64)

### Refactor

- Improve logging
- Pubsub and content topic - gossip agent clean up (#63)
- Improve boot node logic
- Improve error handling
- Rename agent to graphcast (#85)
- Remove redundant log
- Check topics upon receive (#103)
- Log filters and new error variants (#108)
- Ensure unique messages remote and local (#109)
- New label sections (#124)

### Dep

- Revert path to git commit of waku-bindings 0.0.1-beta3 (#93)

<!-- generated by git-cliff -->
