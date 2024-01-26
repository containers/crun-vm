# trust-dns-{client,server} not available
# using vendored deps

# RHEL doesn't include the package rust-packaging which provides %%__cargo macro, but EPEL
# does. So we set it separately here and skip rust-packaging dependency for RHEL.
# Buildability without EPEL is essential for packit builds.
# ELN doesn't need this.
%if %{defined rhel} && 0%{?rhel} < 10
%define __cargo %{_bindir}/env CARGO_HOME=.cargo RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Copt-level=3 -Cdebuginfo=2 -Ccodegen-units=1 -Clink-arg=-Wl,-z,relro -Clink-arg=-Wl,-z,now --cap-lints=warn' %{_bindir}/cargo
%endif

%if %{defined copr_username}
%define copr_build 1
%endif

%global with_debug 1

%if 0%{?with_debug}
%global _find_debuginfo_dwz_opts %{nil}
%global _dwz_low_mem_die_limit 0
%else
%global debug_package %{nil}
%endif

Name: crun-vm
%if %{defined copr_build}
Epoch: 102
%endif
# DO NOT TOUCH the Version string!
# The TRUE source of this specfile is:
# https://github.com/containers/crun-vm/blob/main/rpm/crun-vm.spec
# If that's what you're reading, Version must be 0, and will be updated by Packit for
# copr and koji builds.
# If you're reading this on dist-git, the version is automatically filled in by Packit.
Version: 0
# The `AND` needs to be uppercase in the License for SPDX compatibility
License: (Apache-2.0 OR BSL-1.0) AND (Apache-2.0 OR MIT) AND (MIT OR Apache-2.0) AND (MIT OR Unlicense) AND Apache-2.0 AND GPL-2.0-or-later AND MIT AND MPL-2.0 AND MPL-2.0+ AND Unicode-DFS-2016
Release: %autorelease
%if %{defined golang_arches_future}
ExclusiveArch: %{golang_arches_future}
%else
ExclusiveArch: aarch64 ppc64le s390x x86_64
%endif
Summary: OCI Runtime to run QEMU-compatible VM images
URL: https://github.com/containers/%{name}
# Tarballs fetched from upstream's release page
Source0: %{url}/archive/%{version}.tar.gz
BuildRequires: cargo
BuildRequires: git-core
BuildRequires: libselinux-devel
BuildRequires: make
%if %{defined rhel}
# rust-toolset requires the `local` repo enabled on non-koji ELN build environments
BuildRequires: rust-toolset
%else
BuildRequires: rust-packaging
BuildRequires: rust-srpm-macros
%endif

%description
%{summary}

%prep
%autosetup -Sgit -n %{name}-%{version}

%build
%{__make} CARGO="%{__cargo}" build

%install
%{__make} DESTDIR=%{buildroot} PREFIX=%{_prefix} install

%files
%license LICENSE
%{_bindir}/%{name}

%changelog
%if %{defined autochangelog}
%autochangelog
%else
# NOTE: This changelog will be visible on CentOS Stream 8/9 builds
# Other envs are capable of handling autochangelog
* Mon Jan 16 2024 RH Container Bot <rhcontainerbot@fedoraproject.org>
- Placeholder changelog for envs that are not autochangelog-ready
- Contact upstream if you need to report an issue with the build.
%endif
