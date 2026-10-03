#!/bin/bash
# Build Pong, pack it into a small FAT image and boot it in QEMU.
set -euo pipefail

EFI=target/x86_64-unknown-uefi/release/pong.efi
# OVMF (UEFI firmware) location; override with OVMF=... ./run.sh.
# Debian/Ubuntu: /usr/share/OVMF/OVMF_CODE.fd
OVMF="${OVMF:-/usr/share/edk2/x64/OVMF.4m.fd}"

cargo build --release --target x86_64-unknown-uefi

# One-time setup: a 2 MB FAT image with the default UEFI boot path.
if [ ! -f disk.img ]; then
    dd if=/dev/zero of=disk.img bs=1M count=2
    mformat -i disk.img ::
    mmd -i disk.img ::/efi
    mmd -i disk.img ::/efi/boot
fi

mcopy -o -i disk.img "$EFI" ::/efi/boot/bootx64.efi

qemu-system-x86_64 \
    -bios "$OVMF" \
    -drive format=raw,file=disk.img