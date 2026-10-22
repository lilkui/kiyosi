cmake_minimum_required(VERSION 3.28)

project(kiyosi VERSION 0.12.0 LANGUAGES CXX)

include(GenerateExportHeader)

if(PROJECT_IS_TOP_LEVEL)
    include(CTest)
    option(BUILD_SHARED_LIBS "Build shared libraries" OFF)
endif()
option(KIYOSI_INSTALL "Enable install rules" ${PROJECT_IS_TOP_LEVEL})
option(KIYOSI_ENABLE_CLANG_TIDY "Run clang-tidy while compiling project targets" OFF)
option(KIYOSI_ENABLE_CLANG_FORMAT "Add clang-format and clang-format-check targets" OFF)
option(KIYOSI_ENABLE_RUFF "Add Ruff lint and format checks" OFF)
option(KIYOSI_ENABLE_SANITIZERS "Enable AddressSanitizer and UndefinedBehaviorSanitizer" OFF)
option(KIYOSI_BUILD_PYTHON "Build the Python bindings" OFF)
option(KIYOSI_BUILD_BENCHMARKS "Build Google Benchmark targets" OFF)
option(KIYOSI_BUILD_DOCS "Build Doxygen XML documentation" OFF)
option(KIYOSI_ENABLE_CUDA "Build the optional CUDA Monte Carlo backend" OFF)

include(cmake/OneTBB.cmake)

if(KIYOSI_ENABLE_CUDA)
    include(CheckLanguage)
    check_language(CUDA)
    if(NOT CMAKE_CUDA_COMPILER)
        message(FATAL_ERROR "KIYOSI_ENABLE_CUDA requires a CUDA compiler")
    endif()
    enable_language(CUDA)
    find_package(CUDAToolkit 11.2 REQUIRED)
endif()

add_library(kiyosi_warnings INTERFACE)
add_library(kiyosi::warnings ALIAS kiyosi_warnings)
if(CMAKE_CXX_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC")
    # /utf-8 stops MSVC from decoding non-ASCII sources with the machine's ANSI code page.
    target_compile_options(kiyosi_warnings INTERFACE
        "$<$<COMPILE_LANGUAGE:CXX>:/W4;/permissive-;/utf-8;/Zc:__cplusplus>"
        "$<$<COMPILE_LANGUAGE:CUDA>:-Xcompiler=/utf-8>"
    )
else()
    target_compile_options(kiyosi_warnings INTERFACE
        "$<$<COMPILE_LANG_AND_ID:CXX,GNU,Clang,AppleClang,Intel,IntelLLVM>:-Wall;-Wextra;-Wpedantic>"
    )
endif()

add_library(kiyosi_sanitizers INTERFACE)
add_library(kiyosi::sanitizers ALIAS kiyosi_sanitizers)
if(KIYOSI_ENABLE_SANITIZERS)
    if(NOT CMAKE_CXX_COMPILER_ID MATCHES "^(GNU|Clang|AppleClang)$"
       OR CMAKE_CXX_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC")
        message(FATAL_ERROR "KIYOSI_ENABLE_SANITIZERS requires GCC or Clang with a GNU-style frontend")
    endif()
    target_compile_options(kiyosi_sanitizers INTERFACE
        "$<$<COMPILE_LANG_AND_ID:CXX,GNU,Clang,AppleClang>:-fsanitize=address,undefined;-fno-omit-frame-pointer>"
    )
    target_link_options(kiyosi_sanitizers INTERFACE
        "$<$<LINK_LANG_AND_ID:CXX,GNU,Clang,AppleClang>:-fsanitize=address,undefined>"
    )
endif()

if(KIYOSI_ENABLE_CLANG_TIDY)
    find_program(KIYOSI_CLANG_TIDY_EXECUTABLE NAMES clang-tidy REQUIRED)
endif()

if(KIYOSI_ENABLE_CLANG_FORMAT)
    find_program(KIYOSI_CLANG_FORMAT_EXECUTABLE NAMES clang-format REQUIRED)
    file(GLOB_RECURSE KIYOSI_CLANG_FORMAT_SOURCES CONFIGURE_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/benchmarks/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/benchmarks/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/python/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/python/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cu"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp"
    )
    add_custom_target(kiyosi-format
        COMMAND "${KIYOSI_CLANG_FORMAT_EXECUTABLE}" -i ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        VERBATIM
        USES_TERMINAL
    )
    add_custom_target(kiyosi-format-check
        COMMAND "${KIYOSI_CLANG_FORMAT_EXECUTABLE}" --dry-run --Werror ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        VERBATIM
        USES_TERMINAL
    )
endif()

if(KIYOSI_ENABLE_RUFF)
    find_program(KIYOSI_RUFF_EXECUTABLE NAMES ruff REQUIRED)
    add_custom_target(kiyosi-ruff-check
        COMMAND "${KIYOSI_RUFF_EXECUTABLE}" check .
        COMMAND "${KIYOSI_RUFF_EXECUTABLE}" format --check .
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        VERBATIM
        USES_TERMINAL
    )
endif()

add_library(kiyosi)
add_library(kiyosi::kiyosi ALIAS kiyosi)
target_link_libraries(kiyosi PRIVATE TBB::tbb)
if(BUILD_SHARED_LIBS)
    kiyosi_copy_tbb_runtime(kiyosi)
endif()
generate_export_header(kiyosi
    EXPORT_FILE_NAME "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/kiyosi_export.h"
)
configure_file(
    include/kiyosi/core/version.hpp.in
    "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/core/version.hpp"
    @ONLY
)
target_compile_features(kiyosi PUBLIC cxx_std_23)
target_compile_definitions(kiyosi PRIVATE KIYOSI_HAS_CUDA=$<BOOL:${KIYOSI_ENABLE_CUDA}>)
if(KIYOSI_ENABLE_CUDA)
    target_compile_features(kiyosi PRIVATE cuda_std_20)
    target_link_libraries(kiyosi PRIVATE CUDA::cudart_static)
    set_property(TARGET kiyosi PROPERTY CUDA_RUNTIME_LIBRARY Static)
endif()
if(NOT BUILD_SHARED_LIBS)
    target_compile_definitions(kiyosi PUBLIC KIYOSI_STATIC_DEFINE)
endif()
target_link_libraries(kiyosi PRIVATE $<BUILD_INTERFACE:kiyosi::warnings> $<BUILD_INTERFACE:kiyosi::sanitizers>)
target_sources(kiyosi
    PRIVATE
        src/core/day_count.cpp
        src/core/time.cpp
        src/pricing/detail/instrument_analytics.cpp
        src/market/calendars/sse.cpp
        src/pricing/engines/accumulator/finite_difference.cpp
        src/pricing/engines/accumulator/integral.cpp
        src/pricing/engines/accumulator/monte_carlo.cpp
        src/pricing/engines/asian/analytic.cpp
        src/pricing/engines/barrier/analytic.cpp
        src/pricing/engines/barrier/finite_difference.cpp
        src/pricing/engines/binary_barrier/analytic.cpp
        src/pricing/engines/digital/analytic.cpp
        src/pricing/engines/digital/finite_difference.cpp
        src/pricing/engines/digital/integral.cpp
        src/pricing/engines/autocallable/finite_difference.cpp
        src/pricing/engines/autocallable/integral.cpp
        src/pricing/engines/autocallable/monte_carlo.cpp
        src/pricing/engines/vanilla/analytic.cpp
        src/pricing/engines/vanilla/binomial.cpp
        src/pricing/engines/vanilla/bjerksund_stensland.cpp
        src/pricing/engines/vanilla/finite_difference.cpp
        src/pricing/engines/vanilla/integral.cpp
        src/pricing/engines/vanilla/monte_carlo.cpp
    PUBLIC
        FILE_SET HEADERS
        BASE_DIRS include
        FILES
            include/kiyosi/kiyosi.hpp
            include/kiyosi/core/day_count.hpp
            include/kiyosi/core/error.hpp
            include/kiyosi/core/time.hpp
            include/kiyosi/instruments/accumulator.hpp
            include/kiyosi/instruments/asian.hpp
            include/kiyosi/instruments/digital.hpp
            include/kiyosi/instruments/exercise.hpp
            include/kiyosi/instruments/exercise_based_option.hpp
            include/kiyosi/instruments/option_terms.hpp
            include/kiyosi/instruments/payoff.hpp
            include/kiyosi/instruments/vanilla.hpp
            include/kiyosi/instruments/barrier/binary.hpp
            include/kiyosi/instruments/barrier/option.hpp
            include/kiyosi/instruments/barrier/terms.hpp
            include/kiyosi/instruments/autocallable/fwd.hpp
            include/kiyosi/instruments/autocallable/note.hpp
            include/kiyosi/instruments/autocallable/phoenix.hpp
            include/kiyosi/instruments/autocallable/presets.hpp
            include/kiyosi/instruments/autocallable/snowball.hpp
            include/kiyosi/market/bsm_parameters.hpp
            include/kiyosi/market/calendar.hpp
            include/kiyosi/market/context.hpp
            include/kiyosi/market/schedule.hpp
            include/kiyosi/market/schedule_builders.hpp
            include/kiyosi/market/calendars/sse.hpp
            include/kiyosi/pricing/implied.hpp
            include/kiyosi/pricing/detail/pricing.hpp
            include/kiyosi/pricing/result.hpp
            include/kiyosi/pricing/detail/autocallable_program.hpp
            include/kiyosi/pricing/detail/instrument_analytics.hpp
            include/kiyosi/pricing/detail/autocallable_traits.hpp
            include/kiyosi/pricing/detail/accumulator_program.hpp
            include/kiyosi/pricing/settings/binomial.hpp
            include/kiyosi/pricing/settings/finite_difference.hpp
            include/kiyosi/pricing/settings/implied.hpp
            include/kiyosi/pricing/settings/monte_carlo.hpp
            include/kiyosi/pricing/settings/numerical_shift.hpp
            include/kiyosi/pricing/settings/quadrature.hpp
            include/kiyosi/pricing/engines/accumulator/finite_difference.hpp
            include/kiyosi/pricing/engines/accumulator/integral.hpp
            include/kiyosi/pricing/engines/accumulator/monte_carlo.hpp
            include/kiyosi/pricing/engines/asian/analytic.hpp
            include/kiyosi/pricing/engines/barrier/analytic.hpp
            include/kiyosi/pricing/engines/barrier/finite_difference.hpp
            include/kiyosi/pricing/engines/binary_barrier/analytic.hpp
            include/kiyosi/pricing/engines/digital/analytic.hpp
            include/kiyosi/pricing/engines/digital/finite_difference.hpp
            include/kiyosi/pricing/engines/digital/integral.hpp
            include/kiyosi/pricing/engines/autocallable/finite_difference.hpp
            include/kiyosi/pricing/engines/autocallable/integral.hpp
            include/kiyosi/pricing/engines/autocallable/monte_carlo.hpp
            include/kiyosi/pricing/engines/vanilla/analytic.hpp
            include/kiyosi/pricing/engines/vanilla/binomial.hpp
            include/kiyosi/pricing/engines/vanilla/bjerksund_stensland.hpp
            include/kiyosi/pricing/engines/vanilla/finite_difference.hpp
            include/kiyosi/pricing/engines/vanilla/integral.hpp
            include/kiyosi/pricing/engines/vanilla/monte_carlo.hpp
        FILE_SET generated_headers TYPE HEADERS
        BASE_DIRS "${CMAKE_CURRENT_BINARY_DIR}/generated"
        FILES
            "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/kiyosi_export.h"
            "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/core/version.hpp"
)
if(KIYOSI_ENABLE_CUDA)
    target_sources(kiyosi PRIVATE src/pricing/engines/monte_carlo_cuda.cu)
endif()
set_target_properties(kiyosi PROPERTIES
    CXX_EXTENSIONS OFF
    CXX_SCAN_FOR_MODULES OFF
    DEBUG_POSTFIX d
    POSITION_INDEPENDENT_CODE ON
    # During 0.x, minor versions define ABI compatibility; switch to major-only at 1.0.
    SOVERSION ${PROJECT_VERSION_MAJOR}.${PROJECT_VERSION_MINOR}
    VERSION ${PROJECT_VERSION}
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN YES
)
if(UNIX AND NOT APPLE AND BUILD_SHARED_LIBS)
    set_property(TARGET kiyosi APPEND PROPERTY INSTALL_RPATH "$ORIGIN")
endif()

if(KIYOSI_BUILD_DOCS)
    find_package(Doxygen REQUIRED COMPONENTS doxygen)
    set(DOXYGEN_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/docs")
    set(DOXYGEN_GENERATE_XML YES)
    set(DOXYGEN_GENERATE_HTML NO)
    set(DOXYGEN_GENERATE_LATEX NO)
    set(DOXYGEN_EXTRACT_ALL YES)
    set(DOXYGEN_FILE_PATTERNS "*.hpp")
    set(DOXYGEN_MACRO_EXPANSION YES)
    set(DOXYGEN_EXPAND_ONLY_PREDEF YES)
    set(DOXYGEN_PREDEFINED "KIYOSI_EXPORT=")
    set(DOXYGEN_QUIET YES)
    doxygen_add_docs(kiyosi-doxygen
        "${CMAKE_CURRENT_SOURCE_DIR}/include"
        "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/core/version.hpp"
        ALL
        COMMENT "Generating kiyosi Doxygen XML documentation"
    )
endif()

if(PROJECT_IS_TOP_LEVEL AND BUILD_TESTING)
    include(FetchContent)
    FetchContent_Declare(
        Catch2
        GIT_REPOSITORY https://github.com/catchorg/Catch2.git
        GIT_TAG v3.16.0
        SYSTEM
        EXCLUDE_FROM_ALL
        FIND_PACKAGE_ARGS 3 CONFIG
    )
    FetchContent_MakeAvailable(Catch2)

    add_subdirectory(tests)
endif()

if(KIYOSI_BUILD_BENCHMARKS)
    find_package(QuantLib CONFIG QUIET)
    include(FetchContent)
    set(BENCHMARK_ENABLE_TESTING OFF CACHE BOOL "" FORCE)
    FetchContent_Declare(
        benchmark
        GIT_REPOSITORY https://github.com/google/benchmark.git
        GIT_TAG v1.9.5
        SYSTEM
        EXCLUDE_FROM_ALL
        FIND_PACKAGE_ARGS 1.9.5 CONFIG
    )
    FetchContent_MakeAvailable(benchmark)
    if(benchmark_SOURCE_DIR AND CMAKE_CXX_COMPILER_ID STREQUAL "Clang" AND CMAKE_CXX_SIMULATE_ID STREQUAL "MSVC")
        # Google Benchmark 1.9.5 omits its printf attribute for Clang targeting MSVC.
        target_compile_options(benchmark PRIVATE -Wno-missing-format-attribute -Wno-format-nonliteral)
    endif()
    add_subdirectory(benchmarks)
endif()

if(KIYOSI_ENABLE_CLANG_TIDY)
    set(KIYOSI_TIDY_TARGETS kiyosi)
    if(TARGET kiyosi_benchmarks)
        list(APPEND KIYOSI_TIDY_TARGETS kiyosi_benchmarks)
    endif()
    if(TARGET kiyosi_tests)
        list(APPEND KIYOSI_TIDY_TARGETS kiyosi_tests kiyosi_barrier_assignment_tests)
    endif()
    foreach(target_name IN LISTS KIYOSI_TIDY_TARGETS)
        set_target_properties(${target_name} PROPERTIES CXX_CLANG_TIDY "${KIYOSI_CLANG_TIDY_EXECUTABLE}")
    endforeach()
endif()

if(KIYOSI_BUILD_PYTHON)
    if(BUILD_SHARED_LIBS)
        message(FATAL_ERROR
            "KIYOSI_BUILD_PYTHON requires BUILD_SHARED_LIBS=OFF: the wheel ships only the "
            "extension module, so the core library must be linked into it statically.")
    endif()

    find_package(Python COMPONENTS Interpreter Development.Module REQUIRED)
    find_package(nanobind CONFIG QUIET)
    if(NOT nanobind_FOUND)
        execute_process(
            COMMAND "${Python_EXECUTABLE}" -m nanobind --cmake_dir
            OUTPUT_VARIABLE nanobind_ROOT
            OUTPUT_STRIP_TRAILING_WHITESPACE
            COMMAND_ERROR_IS_FATAL ANY
        )
        find_package(nanobind CONFIG REQUIRED)
    endif()
    if(nanobind_VERSION VERSION_LESS 2.7)
        message(FATAL_ERROR "nanobind 2.7 or newer is required (found ${nanobind_VERSION}).")
    endif()

    nanobind_add_module(_native
        python/kiyosi/_native.cpp
        python/kiyosi/_market.cpp
        python/kiyosi/_instruments.cpp
        python/kiyosi/_autocallable.cpp
        python/kiyosi/_pricing.cpp
    )
    target_link_libraries(_native PRIVATE kiyosi::kiyosi)
    kiyosi_copy_tbb_runtime(_native)
    if(APPLE)
        set_property(TARGET _native PROPERTY INSTALL_RPATH "@loader_path")
    elseif(UNIX)
        set_property(TARGET _native PROPERTY INSTALL_RPATH "$ORIGIN")
    endif()
    if(KIYOSI_ENABLE_CLANG_TIDY)
        # Keep compile commands usable by clang-tidy before the first build creates module maps.
        set_target_properties(_native PROPERTIES
            CXX_SCAN_FOR_MODULES OFF
            CXX_CLANG_TIDY "${KIYOSI_CLANG_TIDY_EXECUTABLE}"
        )
    endif()

    add_custom_command(
        OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/choice_aliases.stubgen"
        COMMAND "${Python_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tools/generate_choice_stubs.py"
            $<TARGET_FILE_DIR:_native> "${CMAKE_CURRENT_BINARY_DIR}/choice_aliases.stubgen"
        DEPENDS _native "${CMAKE_CURRENT_SOURCE_DIR}/tools/generate_choice_stubs.py"
        VERBATIM
    )
    nanobind_add_stub(kiyosi_python_stubs
        MODULE _native
        OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/_native.pyi"
        PYTHON_PATH $<TARGET_FILE_DIR:_native>
        DEPENDS _native
        PATTERN_FILE
            "${CMAKE_CURRENT_SOURCE_DIR}/python/kiyosi/_native.stubgen"
            "${CMAKE_CURRENT_BINARY_DIR}/choice_aliases.stubgen"
    )

    include(CheckIPOSupported)
    check_ipo_supported(RESULT KIYOSI_IPO_SUPPORTED)
    if(KIYOSI_IPO_SUPPORTED)
        set_target_properties(kiyosi _native PROPERTIES
            INTERPROCEDURAL_OPTIMIZATION_RELEASE ON
            INTERPROCEDURAL_OPTIMIZATION_RELWITHDEBINFO ON
        )
    endif()

    install(TARGETS _native
        LIBRARY DESTINATION kiyosi
        RUNTIME DESTINATION kiyosi
    )
    if(WIN32)
        install(FILES $<TARGET_FILE:TBB::tbb> DESTINATION kiyosi)
    else()
        install(FILES $<TARGET_FILE:TBB::tbb> DESTINATION kiyosi
            RENAME $<TARGET_SONAME_FILE_NAME:TBB::tbb>)
    endif()
    install(FILES "${CMAKE_CURRENT_SOURCE_DIR}/cmake/licenses/oneTBB.txt"
        DESTINATION kiyosi/licenses)
    install(FILES "${CMAKE_CURRENT_BINARY_DIR}/_native.pyi"
        DESTINATION kiyosi
    )
endif()

if(KIYOSI_INSTALL)
    include(CMakePackageConfigHelpers)
    include(GNUInstallDirs)
    install(TARGETS kiyosi
        EXPORT kiyosiTargets
        ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}" COMPONENT Development
        LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}" COMPONENT Runtime
            NAMELINK_COMPONENT Development
        RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}" COMPONENT Runtime
        FILE_SET HEADERS DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}" COMPONENT Development
        FILE_SET generated_headers DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}" COMPONENT Development
    )
    install(FILES LICENSE.txt
        DESTINATION "${CMAKE_INSTALL_DATADIR}/licenses/kiyosi"
        COMPONENT Runtime
    )
    write_basic_package_version_file(
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake"
        # Before 1.0, minor versions define compatibility rather than major versions.
        COMPATIBILITY SameMinorVersion
    )
    configure_package_config_file(
        "${CMAKE_CURRENT_SOURCE_DIR}/cmake/kiyosiConfig.cmake.in"
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake"
        INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi"
    )
    install(EXPORT kiyosiTargets
        FILE kiyosiTargets.cmake
        NAMESPACE kiyosi::
        DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi"
        COMPONENT Development
    )
    install(FILES
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake"
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake"
        DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi"
        COMPONENT Development
    )
endif()
