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
  - [x] dhcp4-overrides
    - [x] use-dns
    - [x] use-ntp
    - [x] send-hostname
    - [x] use-hostname
    - [x] use-mtu
    - [x] hostname
    - [x] use-routes
    - [x] route-metric
    - [x] use-domains
  - [x] dhcp6-overrides
    - [x] use-dns
    - [x] use-ntp
    - [x] send-hostname
    - [x] use-hostname
    - [x] hostname
    - [x] use-domains
  - [x] addresses
  - [x] gateway4
  - [x] gateway6
  - [ ] optional
  - [x] mtu
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
