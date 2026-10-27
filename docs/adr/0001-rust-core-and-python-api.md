# Rust 领域核心与 Python API

项目以 Rust 作为长期维护语言，目标是改善代码组织与可维护性，定价准确性是硬约束。目标架构由 Rust 核心独占领域语义，对外提供惯用的 Rust API 与 Python API，Python 适配层保留主要使用方式，并允许为清晰性调整接口。交付范围不包含公开 C++ API 或其兼容层，以避免维护额外的语言接口。

Rust 核心须能独立构建、测试和使用，无需 Python 或绑定框架。交付平台覆盖 Linux、Windows 与 CPython 3.11–3.15。
