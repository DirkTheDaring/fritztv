Name:           fritztv
Version:        0.1.0
Release:        1%{?dist}
Summary:        Fritztv Transcoding Server for FritzBox Cable

License:        MIT
URL:            https://github.com/DirkTheDaring/fritztv
Source0:        %{name}-%{version}.tar.gz
Source1:        config.toml
Source2:        fritztv.service
Source3:        fritztv.sysconfig
Source4:        fritztv.sysusers

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  systemd-rpm-macros

Requires:       /usr/bin/ffmpeg
%{?sysusers_requires_compat}

%description
A transcoding server that interfaces with FritzBox Cable TV tuners to provide
browser-compatible streams via RTSP to fMP4 transcoding.

%global debug_package %{nil}

%prep
%setup -q

%build
# Use a specific target dir to avoid messing with user's dev env if possible, though cargo handles it.
cargo build --release

%install
rm -rf $RPM_BUILD_ROOT
# Install Binary
install -D -p -m 0755 target/release/fritztv %{buildroot}%{_bindir}/fritztv

# Install Config (secure permissions)
install -D -p -m 0640 %{SOURCE1} %{buildroot}%{_sysconfdir}/fritztv/config.toml

# Install Systemd Service
install -D -p -m 0644 %{SOURCE2} %{buildroot}%{_unitdir}/fritztv.service

# Install Sysconfig
install -D -p -m 0644 %{SOURCE3} %{buildroot}%{_sysconfdir}/sysconfig/fritztv

# Install Sysusers
install -D -p -m 0644 %{SOURCE4} %{buildroot}%{_prefix}/lib/sysusers.d/fritztv.conf

# Create Data Directory
mkdir -p %{buildroot}%{_sharedstatedir}/fritztv

%pre
%sysusers_create_compat %{SOURCE4}
exit 0

%post
%systemd_post fritztv.service

%preun
%systemd_preun fritztv.service

%postun
%systemd_postun_with_restart fritztv.service

%files
%license LICENSE
%{_bindir}/fritztv
%dir %{_sysconfdir}/fritztv
%attr(0640, root, fritztv) %config(noreplace) %{_sysconfdir}/fritztv/config.toml
%config(noreplace) %{_sysconfdir}/sysconfig/fritztv
%{_unitdir}/fritztv.service
%{_prefix}/lib/sysusers.d/fritztv.conf
%attr(0750, fritztv, fritztv) %dir %{_sharedstatedir}/fritztv

%changelog
* Tue Jan 06 2026 Dietmar <dietmar@example.com> - 0.1.0-1
- Initial release
