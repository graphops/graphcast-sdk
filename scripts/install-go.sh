#!/usr/bin/env bash

if [ $(uname -m) = "aarch64" ]; then
    wget https://golang.org/dl/go1.20.13.linux-arm64.tar.gz &&
        tar -C /usr/local -xzf go1.20.13.linux-arm64.tar.gz
else
    wget https://golang.org/dl/go1.20.13.linux-amd64.tar.gz &&
        tar -C /usr/local -xzf go1.20.13.linux-amd64.tar.gz
fi
