# NetSprout

Stateless cloud network config generator

## Supported Features

### Data Source

- NoCloud
  - labeled filesystem (iso9660 / vfat + LABEL=CIDATA)

### Renderer

- systemd-networkd

### Network Config v2

- [x] version
- [ ] renderer
- [x] ethernets
  - [x] matches
      - [x] name
      - [x] macaddress
      - [ ] driver
  - [x] set-name
  - [ ] wakeonlan
  - [ ] renderer
  - [x] dhcp4
  - [x] dhcp6
  - [ ] dhcp4-overrides
  - [ ] dhcp6-overrides
  - [x] addresses
  - [x] gateway4
  - [x] gateway6
  - [ ] optional
  - [ ] mtu
  - [x] nameservers
    - [x] search
    - [x] addresses
  - [x] routes
    - [x] to
    - [x] via
    - [x] metric
    - [ ] mtu
    - [ ] table
  - [ ] accept-ra
  - [ ] routing-policy
- [ ] bonds
- [ ] bridges
- [ ] vlans
