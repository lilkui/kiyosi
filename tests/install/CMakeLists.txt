cmake_minimum_required(VERSION 3.28)
project(kiyosi_install_test LANGUAGES CXX)

find_package(kiyosi CONFIG REQUIRED)
add_executable(kiyosi_install_test main.cpp)
target_link_libraries(kiyosi_install_test PRIVATE kiyosi::kiyosi)
set_target_properties(kiyosi_install_test PROPERTIES CXX_SCAN_FOR_MODULES OFF)
enable_testing()
add_test(NAME installed_parallel_mc COMMAND kiyosi_install_test)
