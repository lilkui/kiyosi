add_executable(kiyosi_benchmarks
    vanilla_analytic.cpp
)
set_target_properties(kiyosi_benchmarks PROPERTIES CXX_EXTENSIONS OFF CXX_SCAN_FOR_MODULES OFF)
target_link_libraries(kiyosi_benchmarks PRIVATE kiyosi::kiyosi benchmark::benchmark_main kiyosi::warnings kiyosi::sanitizers)
