# Docker Troubleshooting (Arch Linux)

If Docker fails with messages like:

- `Cannot connect to the Docker daemon`
- `failed to create NAT chain DOCKER`
- `modprobe: FATAL: Module ip_tables not found`

use the steps below.

## 1) Check daemon logs

```bash
sudo journalctl -u docker.service -n 200 --no-pager
sudo journalctl -xeu docker.service --no-pager
```

## 2) Confirm kernel/modules alignment

```bash
uname -r
ls /lib/modules
```

If the running kernel does not match installed modules, Docker networking can fail.

## 3) Install/update required packages

```bash
sudo pacman -Syu linux linux-headers iptables
```

## 4) Reboot

```bash
sudo reboot
```

## 5) After reboot, verify modules and restart services

```bash
uname -r
ls /lib/modules/$(uname -r)
sudo modprobe ip_tables iptable_nat br_netfilter
sudo systemctl restart containerd
sudo systemctl restart docker
docker info
```

## 6) Retry compose

```bash
docker compose up -d postgres
```

## If `modprobe ip_tables` still fails

Your current kernel likely does not include required netfilter modules.

Options:

1. Boot/install a standard Arch `linux` kernel.
2. Use a non-Docker local PostgreSQL setup (documented in `README.md`).
