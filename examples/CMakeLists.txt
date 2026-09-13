add_executable(kiyosi_example_all_pricing_engines all_pricing_engines.cpp)
set_target_properties(kiyosi_example_all_pricing_engines PROPERTIES
    CXX_EXTENSIONS OFF
    CXX_SCAN_FOR_MODULES OFF
    OUTPUT_NAME all_pricing_engines
)
target_link_libraries(kiyosi_example_all_pricing_engines
    PRIVATE kiyosi::kiyosi kiyosi::warnings kiyosi::sanitizers
)
