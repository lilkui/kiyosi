add_executable(kiyosi_benchmarks
    monte_carlo.cpp
    pricing_matrix.cpp
)
set_target_properties(kiyosi_benchmarks PROPERTIES CXX_EXTENSIONS OFF CXX_SCAN_FOR_MODULES OFF)
target_compile_definitions(kiyosi_benchmarks PRIVATE KIYOSI_HAS_CUDA=$<BOOL:${KIYOSI_ENABLE_CUDA}>)
target_link_libraries(kiyosi_benchmarks PRIVATE kiyosi::kiyosi benchmark::benchmark kiyosi::warnings kiyosi::sanitizers)
if(TARGET kiyosi_tests)
    target_sources(kiyosi_tests PRIVATE "${PROJECT_SOURCE_DIR}/tests/benchmarks/pricing_tests.cpp")
    target_link_libraries(kiyosi_tests PRIVATE benchmark::benchmark)
endif()
if(TARGET QuantLib::QuantLib)
    target_compile_definitions(kiyosi_benchmarks PRIVATE KIYOSI_HAS_QUANTLIB=1)
    target_sources(kiyosi_benchmarks PRIVATE quantlib_matrix.cpp)
    target_link_libraries(kiyosi_benchmarks PRIVATE QuantLib::QuantLib)
else()
    target_compile_definitions(kiyosi_benchmarks PRIVATE KIYOSI_HAS_QUANTLIB=0)
    message(STATUS "QuantLib C++ not found; building Kiyosi-only benchmarks")
endif()
