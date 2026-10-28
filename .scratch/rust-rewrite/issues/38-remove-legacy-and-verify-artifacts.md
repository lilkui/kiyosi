# 38: 删除旧实现并验收最终产物

**What to build:** 维护者获得只包含 Rust 核心与 Python 适配的最终工程及可消费产物，旧 C++、CUDA 和失效构建依赖得到清理。

**Blocked by:** 37: 全矩阵验收并统一切换 Rust。

**Status:** completed

- [x] 仅在统一切换及完整验收完成后删除旧 C++ 核心、公开接口、CUDA 内核、旧绑定及其专用构建、安装和分发配置。
- [x] 清理 CUDA 与 oneTBB 等旧实现专用运行依赖和引用；Rust 产物不通过旧 C++/CUDA 桥接层执行领域或数值计算，不提供公开 C++ 或 C ABI 兼容层。
- [x] 保留仍需使用的数值和日历数据及第三方许可；核对 Sobol 方向数、第三方声明和包内许可文件完整。
- [x] 仓库文档、使用示例、构建与测试说明、agent 工作约定、版本与发布配置准确反映 Rust 核心和 CPU 交付状态，无失效入口。
- [x] 在清理后的工程复验独立核心构建与消费、完整 CPU 正确性测试及既定平台和 Python 版本产物，确认删除未破坏此前验收。
- [x] 记录验证结果与环境限制，不设置性能回归门槛；不自动发布正式版本或 Rust crate，也不修改仓库外的文档站点。

## Comments

### 完成核验（2026-10-29）

- 删除前已核对票 37 完成状态与完整切换验收；随后删除旧 C++ 核心/公开头文件、CUDA、nanobind 绑定、CMake 构建/安装/导出配置及其专用测试、基准和工具。不保留公开 C++/C ABI 或领域计算桥接层。
- 独立 QuantLib 模块、JSON、标量差分和仍使用的日历/结构化样例迁至 Rust-backed 测试树；所有参考内容在统一换行后保持一致，数值断言与预算未改变。保留 Rust 日历数据、397,013 个 Sobol 方向数及全部适用许可。
- Windows AMD64 与 Ubuntu 26.04.1 / WSL2 x86-64 的 CPython 3.11.17、3.12.14、3.13.15、3.14.7、3.15.1 普通 wheel 均构建、安装、导入，并在从 sdist 按 CI 配置复制的测试树各通过 444 项测试；早期 3.11/3.12 wheel 更新文档路径后再次通过全套。
- 两平台各通过 389 项 Rust 测试（含 74 项 doctest）、独立核心检查/文档/打包/消费和 Clippy；rustfmt、Ruff、ty、实时日历生成核验通过。Linux Rust 1.85.0 核心构建检查通过。
- 十个 wheel 均完成类型、版本、许可和 native dependency 核验，只有 Python/系统库，无 CUDA、oneTBB 或 C++ 运行库依赖。最终 sdist 补齐测试指南并修正 Markdown glob，其运行时代码/数据/测试/Cargo 输入与完整验收源逐字节一致；两平台从最终 sdist 再构建 CPython 3.15 wheel，重装及包检查通过。
- Standards / Spec 初审和最终补审均无未解决发现。详见 [最终工程与产物验收记录](../../../docs/rust-cleanup.md)，本地产物及 SHA-256 清单位于 `target/ticket38/artifacts`。
- 未运行旧 C++/Python 套件、GitHub 托管 CI 或 manylinux 2.28 容器，未验证 Windows MSRV/完整 MSRV 套件。Linux 本地 wheel 标签为 linux_x86_64，审计兼容 manylinux_2_35；其余环境边界见记录。未设置性能门槛，未发布、推送或修改外部文档站点。
