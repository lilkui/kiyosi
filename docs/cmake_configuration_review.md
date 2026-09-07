## 😀 Cherry 助手

# CMake 配置审查报告 — `kiyosi`

**总体评价**：整体质量相当高——目标级作用域、`FILE_SET HEADERS`、`PROJECT_IS_TOP_LEVEL` 守卫、导出头生成、`install(EXPORT)` 全套打包、pin 到 commit 的 `FetchContent` + `FIND_PACKAGE_ARGS`，都是现代写法。下面按严重程度列出问题。

---

## 一、严重（会导致配置/安装失败或语义错误）

### 1. `PRIVATE` 链接 `kiyosi::warnings` 与 `install(EXPORT)` 冲突

* **问题**：`target_link_libraries(kiyosi PRIVATE kiyosi::warnings)` 中的 `kiyosi_warnings` 未加入任何 export set，但 `install(EXPORT kiyosiTargets)` 会要求它。
* **原因**：CMake 会把 `PRIVATE` 依赖以 `$<LINK_ONLY:kiyosi_warnings>` 形式写入 `INTERFACE_LINK_LIBRARIES`（静态库尤其必须保留，用于传递链接）。导出时报：
  `install(EXPORT "kiyosiTargets" ...) includes target "kiyosi" which requires target "kiyosi_warnings" that is not in any export set.`
  默认 `BUILD_SHARED_LIBS=OFF` + `KIYOSI_INSTALL=ON`（顶层默认）即可复现。
* **建议**（二选一）：

  ```cmake
  # 方案 A：让依赖只存在于 build interface（推荐，警告选项本就不该外泄）
  target_link_libraries(kiyosi PRIVATE $<BUILD_INTERFACE:kiyosi::warnings>)

  # 方案 B：把 warnings 目标一并导出
  install(TARGETS kiyosi_warnings EXPORT kiyosiTargets)
  ```

---

## 二、中等（可维护性 / 正确性隐患）

### 2. 包含目录与 `FILE_SET` 的 `BASE_DIRS` 重复声明

* **问题**：`target_include_directories(kiyosi PUBLIC $<BUILD_INTERFACE:.../include> $<BUILD_INTERFACE:.../generated>)` 以及安装块里的 `$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>` 均为冗余。
* **原因**：`FILE_SET`（HEADERS 类型）的 `BASE_DIRS` 会自动以 `$<BUILD_INTERFACE:>` 加入目标的 include 目录；`install(TARGETS ... FILE_SET ... DESTINATION x)` 会自动追加 `$<INSTALL_INTERFACE:x>`。更糟的是把 `INSTALL_INTERFACE` 写在 `if(KIYOSI_INSTALL)` 内，使**导出目标的接口随选项变化**，属于隐蔽陷阱。
* **建议**：删除这两处 `target_include_directories`，完全依赖 file set；如需保留，也必须移出条件块。

### 3. 可见性设置只在共享库分支生效

* **问题**：`CXX_VISIBILITY_PRESET hidden` / `VISIBILITY_INLINES_HIDDEN` 仅在 `if(BUILD_SHARED_LIBS)` 内设置。
* **原因**：静态库同样可能被链入下游 `.so`，两种构建的符号语义与代码生成不一致，容易出现"共享构建才暴露"的链接错误；隐藏可见性对静态库也无害。
* **建议**：把这两个属性无条件设置，只把 `VERSION`/`SOVERSION` 留在共享分支。

### 4. 用 `BUILD_SHARED_LIBS` 变量代替目标类型判断

* **问题**：`if(BUILD_SHARED_LIBS)` 决定共享库属性。
* **原因**：目标类型是 `add_library(kiyosi)` 求值时确定的；若日后显式写 `add_library(kiyosi STATIC)`，或作为子项目被父项目中途修改变量，两者会脱节。
* **建议**：

  ```cmake
  get_target_property(kiyosi_type kiyosi TYPE)
  if(kiyosi_type STREQUAL "SHARED_LIBRARY")
      ...
  endif()
  ```

### 5. 缺少 "warnings as errors" 开关与 MSVC 必需选项

* **问题**：`kiyosi_warnings` 只有 `/W4`、`-Wall -Wextra -Wpedantic`，没有 `-Werror`/`/WX`，也没有 `/utf-8`、`/Zc:__cplusplus`。
* **原因**：CI 需要"警告即错误"；MSVC 默认按本地代码页读源码（C++23 + UTF-8 字面量易出错），且不开 `/Zc:__cplusplus` 时 `__cplusplus` 恒为 `199711L`，会让特性检测代码走错分支（`/permissive-` 并不隐含该开关）。
* **建议**：新增 `KIYOSI_WARNINGS_AS_ERRORS` 选项（默认 OFF，CI preset 打开），并在 warnings 目标中补：

  ```cmake
  "$<$<COMPILE_LANG_AND_ID:CXX,MSVC>:/utf-8;/Zc:__cplusplus>"
  "$<$<AND:$<BOOL:${KIYOSI_WARNINGS_AS_ERRORS}>,$<COMPILE_LANG_AND_ID:CXX,MSVC>>:/WX>"
  "$<$<AND:$<BOOL:${KIYOSI_WARNINGS_AS_ERRORS}>,$<COMPILE_LANG_AND_ID:CXX,GNU,Clang,AppleClang>>:-Werror>"
  ```

### 6. 测试目标链接非命名空间目标名

* **问题**：`target_link_libraries(kiyosi_tests PRIVATE kiyosi ...)`。
* **原因**：非 `::` 名称若拼错，CMake 会当作系统库 `-lkiyosi` 而**不报配置错误**，退化为链接期报错。
* **建议**：统一用 `kiyosi::kiyosi`。同时建议把整段测试逻辑移入 `tests/CMakeLists.txt`（`add_subdirectory(tests)`），根文件目前承担了库/测试/工具/打包四种职责。

### 7. C++23 + CMake 3.28 的默认模块扫描

* **问题**：未设置 `CXX_SCAN_FOR_MODULES`。
* **原因**：CMake 3.28 对 C++20 及以上标准的目标默认启用 C++ modules 依赖扫描，带来额外构建开销，并对部分工具链组合较脆弱。
* **建议**：若项目不使用 modules，显式关闭：`set_target_properties(kiyosi kiyosi_tests PROPERTIES CXX_SCAN_FOR_MODULES OFF)`。

### 8. clang-format 目标使用 `GLOB_RECURSE CONFIGURE_DEPENDS`

* **问题**：三点——① `CONFIGURE_DEPENDS` 让**每次构建**都要重新 glob，仅为格式化目标付出全局代价；② 模式遗漏 `src/**/*.hpp`、`*.h`、`*.cc`；③ 文件很多时会撞上 Windows 8191 字符命令行上限。
* **建议**：直接复用已有的 `KIYOSI_SOURCES`/`KIYOSI_HEADERS` 显式列表（顺带保证"格式化范围 = 构建范围"），至少去掉 `CONFIGURE_DEPENDS`。

### 9. clang-tidy 集成缺少可用性支撑

* **问题**：仅设置 `CXX_CLANG_TIDY`，未开启 `CMAKE_EXPORT_COMPILE_COMMANDS`，也未限定编译器。
* **原因**：`clang-tidy` 搭配 MSVC `cl` 驱动时参数不兼容（通常需要 `--extra-arg` 或改用 clang-cl）；缺少 `compile_commands.json` 则无法在 CI/编辑器中独立运行 tidy 与 clangd。
* **建议**：`set(CMAKE_EXPORT_COMPILE_COMMANDS ON)`（或在 preset 中打开），并在 MSVC 下给出警告/跳过；另可加 `--warnings-as-errors=*` 由 CI 控制。

---

## 三、轻微 / 风格

| # | 问题 | 原因 | 建议 |
|---|---|---|---|
| 10 | 两个相邻的 `if(PROJECT_IS_TOP_LEVEL)` 块 | 纯冗余 | 合并为一个块 |
| 11 | `option(KIYOSI_INSTALL "..." ${PROJECT_IS_TOP_LEVEL})` 未加引号 | 变量为空时参数个数变化（此处虽由 `project()` 保证非空，但属不良习惯） | 改为 `"${PROJECT_IS_TOP_LEVEL}"` |
| 12 | 全文硬编码 `kiyosi` 字面量（安装路径、导出名、宏前缀） | 重命名成本高、易漏改 | 统一使用 `${PROJECT_NAME}` |
| 13 | LICENSE 安装到 `${CMAKE_INSTALL_DATADIR}/licenses/kiyosi` | 非 GNU 惯例位置 | 使用 `${CMAKE_INSTALL_DOCDIR}`（其已含项目名） |
| 14 | `detail/sse_holidays.hpp` 进入公共 `FILE_SET HEADERS` | `detail/` 属实现细节，安装即成为事实上的公共 API | 移出安装集，或放入独立的私有 file set |
| 15 | 源文件/头文件先 `set()` 再展开 | 现代惯例是直接列在 `target_sources` 中；两份列表需手工同步，新增文件易漏 | 直接内联到 `target_sources`；或保留变量但确保格式化目标复用同一份 |
| 16 | 未提供 build-tree 的 `export(EXPORT kiyosiTargets ...)` | 下游用 `FetchContent`/超级构建时无法直接消费构建树 | 可选补充（有 ALIAS 时优先级较低） |
| 17 | 未见 `cmake/kiyosiConfig.cmake.in` 内容 | 无法验证 `@PACKAGE_INIT@` 与 `check_required_components(kiyosi)` 是否齐备 | 请确认其含 `@PACKAGE_INIT@`、`include(".../kiyosiTargets.cmake")` 与 `check_required_components()` |

---

## 四、`CMakePresets.json`

### 18. sanitizer preset 直接覆盖 `CMAKE_CXX_FLAGS`
* **原因**：`cacheVariables` 中的 `CMAKE_CXX_FLAGS` 会**整体替换**用户/工具链提供的标志；同时只设置了 `CMAKE_EXE_LINKER_FLAGS`，`BUILD_SHARED_LIBS=ON` 时共享库不会带上 sanitizer 运行时。
* **建议**：补 `CMAKE_SHARED_LINKER_FLAGS`、`CMAKE_MODULE_LINKER_FLAGS`；更好的做法是在 CMakeLists 中提供 `KIYOSI_ENABLE_SANITIZERS` 选项，用 `target_compile_options`/`target_link_options` 施加到目标上，preset 只负责打开开关。

### 19. sanitizer 测试 preset 未设置 UBSan 环境变量（**易被忽视但影响正确性**）
* **原因**：UBSan 默认只打印诊断并继续执行，测试仍会"通过"，导致 CI 形同虚设。
* **建议**：在对应 testPreset 中加
  ```json
  "environment": {
    "UBSAN_OPTIONS": "print_stacktrace=1:halt_on_error=1",
    "ASAN_OPTIONS": "abort_on_error=1"
  }
  ```

### 20. 未开启 `CMAKE_EXPORT_COMPILE_COMMANDS`
* **原因**：项目已内建 clang-tidy 支持，clangd/CI 静态分析都依赖 `compile_commands.json`。
* **建议**：在 `linux-base`/`windows-base` 的 `cacheVariables` 中设为 `"ON"`。

### 21. `linux-base` 的 `architecture: {x64, external}` 无实际意义
* **原因**：`strategy: external` 表示由外部环境决定，Ninja + Linux 下该字段被忽略，只是噪音（Windows 侧配合 vsdevcmd 尚有语义）。
* **建议**：从 Linux 预设中移除。

### 22. 预设覆盖面与 CI 支持不足
* **原因**：① 编译选项支持 AppleClang 但无 macOS 预设；② 没有任何预设打开 `KIYOSI_ENABLE_CLANG_TIDY` / `KIYOSI_ENABLE_CLANG_FORMAT`，这些选项在 CI 中实际不可达；③ `binaryDir`/`installDir` 在两个 base 中重复；④ testPresets 未设 `"execution": {"noTestsAction": "error"}`，测试发现失败时会静默通过；⑤ 已用 version 6 却未使用 `workflowPresets` 串联 configure→build→test。
* **建议**：抽取一个公共 hidden `base` 预设；新增 `ci-linux` 类预设开启 tidy/format-check/warnings-as-errors；testPresets 补 `noTestsAction: error` 与 `"jobs"`；顶层补 `workflowPresets`；`configurePresets` 可加 `"warnings": {"dev": true}, "errors": {"deprecated": true}`。

---

## 五、修复优先级

1. **必修**：#1（导出集失败，阻断安装）
2. **高**：#2、#3、#5、#19（正确性/一致性）
3. **中**：#4、#6、#7、#8、#9、#18、#20
4. **低**：其余为整洁性与工程化改进
