Name:           vci-openfactory-platform
Version:        0.1.0
Release:        1
Summary:        VCI forwarding component for OpenFactory host configuration
License:        MIT AND MPL-2.0 AND Apache-2.0 AND BSD-3-Clause
URL:            https://github.com/elster-drones/vci-openfactory-platform
Source0:        vci-openfactory-platform_0.1.0.tar.gz
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  rust-vci-devel >= 0.2
BuildRequires:  rust-serde_json+default-devel
BuildRequires:  rust-tokio+full-devel
Requires:       vci-helper
Requires:       openfactory-storage >= 1.2.0

%description
Forwards Check, Set, config Get and state Get to openfactory-storage with a
bounded deadline and no in-memory settings cache.

%prep
%setup -q -c -T
tar -xf %{SOURCE0} --strip-components=1

%build
export CARGO_HOME=$PWD/target/cargo-home
install -Dpm0644 packaging/cargo-config.toml "$CARGO_HOME/config.toml"
cargo build --release --offline

%check
export CARGO_HOME=$PWD/target/cargo-home
cargo test --offline

%install
install -Dpm0755 target/release/vci-openfactory-platform %{buildroot}%{_libexecdir}/vci/vci-openfactory-platform
install -Dpm0644 debian/vci-openfactory-platform.component %{buildroot}%{_prefix}/lib/vci/components/tech.openfactory.platform.component
install -Dpm0644 packaging/tech.openfactory.platform.service.d/timeout.conf %{buildroot}%{_unitdir}/tech.openfactory.platform.service.d/timeout.conf

%files
%license LICENSE
%doc README.md
%{_libexecdir}/vci/vci-openfactory-platform
%{_prefix}/lib/vci/components/tech.openfactory.platform.component
%{_unitdir}/tech.openfactory.platform.service.d/timeout.conf
