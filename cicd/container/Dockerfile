# Build, lint, test and package tools for 'cicd.bash --container', pinned, so stages
# 1 to 6 do not depend on what the host has installed. Sync, the Windows boxes, the
# private runner, dogfood, the demo and publish stay on the host.
#
# The tool versions come from cicd/tool-pins.txt and rust-toolchain.toml as build
# args, and the image tag hashes this file plus those args. A version bump without
# its checksum here fails the build.
#
# Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
# SPDX-License-Identifier: MIT

FROM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f AS base

ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8

# Trixie is what the build box runs, so the X and Wayland tools, Mesa, mingw and the
# fonts come from apt at the versions the box has. The window tests run on Xvfb under
# xfwm4, or under sway and cage, all on llvmpipe. libicu and libgssapi are for pwsh.
# python3-xlib and Pillow are for the test drivers. shellcheck comes from upstream
# below, since trixie's is 0.10 and its warnings differ.
RUN apt-get update \
	&& apt-get install -y --no-install-recommends \
		bash ca-certificates curl git xz-utils unzip zstd file procps iproute2 \
		python3 python3-venv python3-xlib python3-pil \
		gcc libc6-dev pkg-config gcc-mingw-w64-x86-64 dpkg-dev \
		xvfb xauth x11-utils x11-apps x11-xserver-utils xdotool xfwm4 imagemagick expect \
		sway xwayland cage grim dbus \
		fonts-dejavu-core fonts-noto-color-emoji fonts-noto-cjk \
		libegl1 libgl1 libgl1-mesa-dri libglx-mesa0 mesa-vulkan-drivers libvulkan1 \
		libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-egl1 \
		libx11-6 libx11-xcb1 libxcursor1 libxi6 libxrandr2 libxinerama1 \
		libxcb1 libxcb-xkb1 libxcb-xinput0 libxcb-randr0 libxcb-cursor0 libxcb-render0 \
		libxcb-shape0 libxcb-xfixes0 libxcb-shm0 \
		libicu76 libgssapi-krb5-2 \
	&& rm -rf /var/lib/apt/lists/*

# makensis at its pin. Trixie has 3.11, so the 3.12 package comes from the snapshot
# archive, where it stays put.
ARG VER_makensis
ARG NSIS_SNAP=20260504T084255Z
ARG NSIS_SHA256=7df43fb0dee49146089c4c9cde11621305fe6c978e1654c061c19b7451ffa340
ARG NSIS_COMMON_SHA256=03e1944ea80c1f8f833baafffde085c0c68ccd0dca7ada3a0274fa3d239b9274
WORKDIR /tmp/deb
RUN : "${VER_makensis:?}" \
	&& pool="https://snapshot.debian.org/archive/debian/${NSIS_SNAP}/pool/main/n/nsis" \
	&& curl -fsSLo nsis.deb "${pool}/nsis_${VER_makensis}-1_amd64.deb" \
	&& curl -fsSLo nsis-common.deb "${pool}/nsis-common_${VER_makensis}-1_all.deb" \
	&& printf '%s  %s\n' "${NSIS_SHA256}" nsis.deb "${NSIS_COMMON_SHA256}" nsis-common.deb | sha256sum -c - \
	&& apt-get update \
	&& apt-get install -y --no-install-recommends ./nsis-common.deb ./nsis.deb \
	&& rm -rf /var/lib/apt/lists/* \
	&& cd / && rm -rf /tmp/deb

ARG VER_zig
ARG ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ARG VER_shellcheck
# Upstream posts no checksum; this one is from the download.
ARG SC_SHA256=8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198
ARG PWSH_VER=7.6.6
ARG PWSH_SHA256=ddbc4a2d113bbd46d283cfedcbcd117a70caefd7673f41f2b4e0000badf103bc
ARG PSA_VER=1.25.0
# PSGallery posts no checksum either.
ARG PSA_SHA256=14e634c828eb98efb9f40b2918ba90f139ed5eccdf663a2a747736d996995d60
WORKDIR /tmp/dl
RUN : "${VER_zig:?}${VER_shellcheck:?}" \
	&& curl -fsSLo zig.txz "https://ziglang.org/download/${VER_zig}/zig-x86_64-linux-${VER_zig}.tar.xz" \
	&& echo "${ZIG_SHA256}  zig.txz" | sha256sum -c - \
	&& mkdir /opt/zig && tar -C /opt/zig --strip-components=1 -xJf zig.txz \
	&& curl -fsSLo sc.txz "https://github.com/koalaman/shellcheck/releases/download/v${VER_shellcheck}/shellcheck-v${VER_shellcheck}.linux.x86_64.tar.xz" \
	&& echo "${SC_SHA256}  sc.txz" | sha256sum -c - \
	&& tar -C /usr/local/bin --strip-components=1 -xJf sc.txz "shellcheck-v${VER_shellcheck}/shellcheck" \
	&& curl -fsSLo pwsh.tgz "https://github.com/PowerShell/PowerShell/releases/download/v${PWSH_VER}/powershell-${PWSH_VER}-linux-x64.tar.gz" \
	&& echo "${PWSH_SHA256}  pwsh.tgz" | sha256sum -c - \
	&& mkdir /opt/pwsh && tar -C /opt/pwsh -xzf pwsh.tgz && chmod +x /opt/pwsh/pwsh \
	&& ln -s /opt/pwsh/pwsh /usr/local/bin/pwsh \
	&& curl -fsSLo psa.nupkg "https://www.powershellgallery.com/api/v2/package/PSScriptAnalyzer/${PSA_VER}" \
	&& echo "${PSA_SHA256}  psa.nupkg" | sha256sum -c - \
	&& psa="/usr/local/share/powershell/Modules/PSScriptAnalyzer/${PSA_VER}" \
	&& mkdir -p "${psa}" && unzip -q psa.nupkg -d "${psa}" \
	&& rm -rf "${psa}/_rels" "${psa}/package" "${psa}/[Content_Types].xml" "${psa}/PSScriptAnalyzer.nuspec" \
	&& cd / && rm -rf /tmp/dl

# ruff and mypy at their pins, from PyPI. The venv sees the apt python modules too.
ARG VER_ruff
ARG VER_mypy
RUN : "${VER_ruff:?}${VER_mypy:?}" \
	&& python3 -m venv --system-site-packages /opt/venv \
	&& /opt/venv/bin/pip install --no-cache-dir --disable-pip-version-check "ruff==${VER_ruff}" "mypy==${VER_mypy}"

# The toolchain rust-toolchain.toml names, with its cross targets, under /opt, where a
# run cannot change it. rustup's own CARGO_HOME here only holds the proxies.
ARG VER_rust
ARG RUST_TARGETS
ARG RUSTUP_VER=1.29.1
ARG RUSTUP_SHA256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
WORKDIR /tmp/dl
RUN : "${VER_rust:?}${RUST_TARGETS:?}" \
	&& curl -fsSLo rustup-init "https://static.rust-lang.org/rustup/archive/${RUSTUP_VER}/x86_64-unknown-linux-gnu/rustup-init" \
	&& echo "${RUSTUP_SHA256}  rustup-init" | sha256sum -c - \
	&& chmod +x rustup-init \
	&& targets="" && for t in ${RUST_TARGETS}; do targets="${targets} -t ${t}"; done \
	&& ./rustup-init -y --no-modify-path --profile minimal --default-toolchain "${VER_rust}" -c rustfmt -c clippy ${targets} \
	&& cd / && rm -rf /tmp/dl

# The cargo helpers are built from source in their own stage, so the final image
# gets the binaries and none of the build cache.
FROM base AS tools
ARG JOBS=8
ARG VER_cargo_deny
ARG VER_cargo_zigbuild
ARG VER_cargo_deb
ARG VER_cargo_generate_rpm
RUN : "${VER_cargo_deny:?}${VER_cargo_zigbuild:?}${VER_cargo_deb:?}${VER_cargo_generate_rpm:?}" \
	&& /opt/cargo/bin/cargo install --locked -j "${JOBS}" --root /opt/tools \
		"cargo-deny@${VER_cargo_deny}" "cargo-zigbuild@${VER_cargo_zigbuild}" \
		"cargo-deb@${VER_cargo_deb}" "cargo-generate-rpm@${VER_cargo_generate_rpm}"

FROM base
COPY --from=tools /opt/tools/bin/ /opt/cargo/bin/

# Runs as the caller's uid, which has no passwd entry here. /cache takes the named
# volume, which copies this dir's mode when new, so anyone can write. The crate
# registry goes there, so the toolchain under /opt stays as the image built it, and
# rustup is told not to fetch a toolchain the image lacks.
RUN mkdir -m 1777 /cache /cache/home /cache/cargo
ENV PATH=/opt/cargo/bin:/opt/zig:/opt/venv/bin:${PATH} \
	HOME=/cache/home \
	CARGO_HOME=/cache/cargo \
	RUSTUP_AUTO_INSTALL=0 \
	SILK_CICD_IN_CONTAINER=1 \
	POWERSHELL_TELEMETRY_OPTOUT=1 \
	POWERSHELL_UPDATECHECK=Off \
	DOTNET_CLI_TELEMETRY_OPTOUT=1
WORKDIR /
