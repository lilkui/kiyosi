add_executable(kiyosi_tests
    api/analytics_headers_tests.cpp
    api/header_tests.cpp
    api/parity_tests.cpp
    instruments/history_replace_tests.cpp
    instruments/accumulator_tests.cpp
    instruments/asian_tests.cpp
    instruments/digital_tests.cpp
    instruments/vanilla_tests.cpp
    instruments/barrier/binary_tests.cpp
    instruments/barrier/option_tests.cpp
    instruments/barrier/terms_tests.cpp
    instruments/autocallable/snowball_tests.cpp
    market/calendar_tests.cpp
    market/context_tests.cpp
    pricing/integration_tests.cpp
    pricing/invariants_tests.cpp
    pricing/analytics/api_contract_tests.cpp
    pricing/analytics/audit_tests.cpp
    pricing/analytics/discount_tests.cpp
    pricing/analytics/architecture_tests.cpp
    pricing/analytics/numerical_validation_tests.cpp
    pricing/detail/fd_scheme_tests.cpp
    pricing/detail/fd_event_tests.cpp
    pricing/engines/accumulator/engine_tests.cpp
    pricing/engines/accumulator/quadrature_tests.cpp
    pricing/engines/asian/reference_tests.cpp
    pricing/engines/barrier/analytic_tests.cpp
    pricing/engines/barrier/finite_difference_tests.cpp
    pricing/engines/barrier/reference_tests.cpp
    pricing/engines/binary_barrier/analytic_tests.cpp
    pricing/engines/binary_barrier/reference_tests.cpp
    pricing/engines/digital/settlement_tests.cpp
    pricing/engines/digital/overflow_tests.cpp
    pricing/engines/autocallable/autocallable_tests.cpp
    pricing/engines/autocallable/quadrature_tests.cpp
    pricing/engines/autocallable/accuracy_tests.cpp
    pricing/engines/autocallable/validation_tests.cpp
    pricing/engines/vanilla/analytic_tests.cpp
    pricing/engines/vanilla/binomial_tests.cpp
    pricing/engines/vanilla/finite_difference_tests.cpp
    pricing/engines/vanilla/monte_carlo_tests.cpp
    reference/generated_tests.cpp
    reference/parser_tests.cpp
)
set_target_properties(kiyosi_tests PROPERTIES CXX_EXTENSIONS OFF CXX_SCAN_FOR_MODULES OFF)
target_link_libraries(kiyosi_tests PRIVATE kiyosi::kiyosi TBB::tbb Catch2::Catch2WithMain kiyosi::warnings kiyosi::sanitizers)
kiyosi_copy_tbb_runtime(kiyosi_tests)
if(KIYOSI_ENABLE_CUDA)
    target_link_libraries(kiyosi_tests PRIVATE CUDA::cudart_static)
    target_sources(kiyosi_tests PRIVATE pricing/engines/monte_carlo_cuda_tests.cu)
endif()
target_include_directories(kiyosi_tests PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}" "${PROJECT_SOURCE_DIR}/src")
target_compile_definitions(kiyosi_tests PRIVATE
    KIYOSI_SOURCE_DIR="${PROJECT_SOURCE_DIR}"
    KIYOSI_HAS_CUDA=$<BOOL:${KIYOSI_ENABLE_CUDA}>
)
# Only set when FetchContent built Catch2 from source; an installed package ships Catch.cmake itself.
if(catch2_SOURCE_DIR)
    list(APPEND CMAKE_MODULE_PATH "${catch2_SOURCE_DIR}/extras")
endif()
include(Catch)
catch_discover_tests(kiyosi_tests
    DISCOVERY_MODE PRE_TEST
    DL_PATHS $<TARGET_FILE_DIR:kiyosi>
)
