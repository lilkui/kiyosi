# 生成 C++ API XML

安装 Doxygen 后，可通过 CMake 生成供文档工具使用的 XML。Windows 下请先打开 Visual Studio Developer PowerShell，然后在仓库根目录运行：

```sh
cmake -S . -B build/docs -DBUILD_TESTING=OFF -DKIYOSI_BUILD_DOCS=ON
cmake --build build/docs --target kiyosi-doxygen
```

入口文件为 `build/docs/docs/xml/index.xml`，内容来自 C++ 头文件及生成的版本头文件。启用后，默认构建也会生成 XML；每次构建 `kiyosi-doxygen` 都会重新生成。

`KIYOSI_BUILD_DOCS` 默认关闭，普通构建无需 Doxygen、Graphviz 或 Python。启用后需要 Doxygen，但无需 Graphviz 或 Python，也无需编译核心库即可运行文档目标。
