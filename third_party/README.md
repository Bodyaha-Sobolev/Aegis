# Сторонние компоненты

iced-x86 1.21.0 — временное ядро декодирования/дизассемблирования x86/x64.
Исходники crates.io сохранены в iced-x86 без изменения реализации.
Оригинальные уведомления в исходниках и LICENSE.txt сохранены.

Upstream: https://github.com/icedland/iced
Tag: v1.21.0
Commit: c50f29b7bc305696895c075f3fc7719751426b12
Package: https://crates.io/crates/iced-x86/1.21.0
Archive SHA256: 7c447cff8c7f384a7d4f741cfcff32f75f3ad02b406432e8d6c878d56b1edf6b
License: MIT; текст взят из https://raw.githubusercontent.com/icedland/iced/v1.21.0/LICENSE.txt

Адаптер Aegis будет отдельным модулем; vendor-код не является собственной
реализацией Aegis. Позже его можно заменить с сохранением API и тестов.
При изменении vendor-кода записывать локальный diff и исходную версию.

Локальное изменение только Cargo.toml: разрешены предупреждения
unexpected_cfgs и unused_attributes для совместимости с Rust 1.96.1
(исторические cfg cargo-fmt и #[must_use] в trait impl). Исходные .rs не менялись.
Форматирование и lint Aegis выполняются для трёх собственных workspace crates;
vendor исключён из workspace. Crates.io-пакет не содержит upstream test modules,
поэтому cargo fmt --all не используется: он пытается обходить и path-dependencies.
