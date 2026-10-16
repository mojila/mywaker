Name:           mywaker
Version:        @VERSION@
Release:        1%{?dist}
Summary:        Run a team of AI employees backed by OpenCode from your terminal
License:        MIT
URL:            https://github.com/mojila/mywaker

%description
mywaker is a Rust TUI that turns OpenCode into a roster of on-screen AI
employees, with an animated robot avatar and cross-provider model routing.

%install
mkdir -p %{buildroot}%{_prefix}/local/bin
install -m 0755 %{_topdir}/SOURCES/mywaker %{buildroot}%{_prefix}/local/bin/mywaker

%files
%{_prefix}/local/bin/mywaker

%changelog
* Fri Oct 16 2026 mywaker contributors - @VERSION@-1
- Initial package.