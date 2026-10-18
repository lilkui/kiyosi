include(FetchContent)

function(kiyosi_find_tbb)
    find_package(TBB 2023.1 CONFIG QUIET COMPONENTS tbb)
    if(NOT TBB_FOUND)
        # oneTBB requires a shared runtime even when kiyosi itself is static.
        set(BUILD_SHARED_LIBS ON)
        set(TBB_TEST OFF CACHE BOOL "" FORCE)
        set(TBB_EXAMPLES OFF CACHE BOOL "" FORCE)
        set(TBBMALLOC_BUILD OFF CACHE BOOL "" FORCE)
        set(TBBMALLOC_PROXY_BUILD OFF CACHE BOOL "" FORCE)
        set(TBB_STRICT OFF CACHE BOOL "" FORCE)
        set(TBB_INSTALL ${KIYOSI_INSTALL} CACHE BOOL "" FORCE)
        FetchContent_Declare(oneTBB
            GIT_REPOSITORY https://github.com/uxlfoundation/oneTBB.git
            GIT_TAG v2023.1.0
            SYSTEM
        )
        FetchContent_MakeAvailable(oneTBB)
    endif()
endfunction()

kiyosi_find_tbb()

# Windows does not use build RPATHs; each executable/module needs its runtime beside it.
function(kiyosi_copy_tbb_runtime target)
    if(WIN32)
        add_custom_command(TARGET ${target} POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                $<TARGET_FILE:TBB::tbb> $<TARGET_FILE_DIR:${target}>
            VERBATIM
        )
    endif()
endfunction()
