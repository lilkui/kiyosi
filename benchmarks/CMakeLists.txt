add_executable(kiyosi_benchmarks
    monte_carlo.cpp
    pricing_matrix.cpp
)
set_target_properties(kiyosi_benchmarks PROPERTIES CXX_EXTENSIONS OFF CXX_SCAN_FOR_MODULES OFF)
target_compile_definitions(kiyosi_benchmarks PRIVATE KIYOSI_HAS_CUDA=$<BOOL:${KIYOSI_ENABLE_CUDA}>)
target_link_libraries(kiyosi_benchmarks PRIVATE kiyosi::kiyosi benchmark::benchmark_main kiyosi::warnings kiyosi::sanitizers)
if(TARGET QuantLib::QuantLib)
    target_sources(kiyosi_benchmarks PRIVATE quantlib_matrix.cpp)
    target_link_libraries(kiyosi_benchmarks PRIVATE QuantLib::QuantLib)
else()
    message(STATUS "QuantLib C++ not found; building Kiyosi-only benchmarks")
endif()
