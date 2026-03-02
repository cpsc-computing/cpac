cmake_minimum_required(VERSION 3.15)
project(cpac-ffi VERSION 0.1.0 LANGUAGES C CXX)

# Options
option(CPAC_BUILD_STATIC "Build static library" ON)
option(CPAC_BUILD_SHARED "Build shared library" ON)
option(CPAC_GENERATE_HEADER "Generate C header with cbindgen" ON)

# Rust target directory (relative to this CMakeLists.txt)
set(CARGO_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../target")
set(RUST_BUILD_TYPE "release")

# Determine library names based on platform
if(WIN32)
    set(STATIC_LIB_NAME "cpac_ffi.lib")
    set(SHARED_LIB_NAME "cpac_ffi.dll")
    set(SHARED_IMPLIB_NAME "cpac_ffi.dll.lib")
elseif(APPLE)
    set(STATIC_LIB_NAME "libcpac_ffi.a")
    set(SHARED_LIB_NAME "libcpac_ffi.dylib")
else()
    set(STATIC_LIB_NAME "libcpac_ffi.a")
    set(SHARED_LIB_NAME "libcpac_ffi.so")
endif()

# Build Rust library
add_custom_target(cpac-rust-build ALL
    COMMAND cargo build --release
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../..
    COMMENT "Building CPAC Rust library..."
)

# Generate C header with cbindgen (optional)
if(CPAC_GENERATE_HEADER)
    find_program(CBINDGEN cbindgen)
    if(CBINDGEN)
        add_custom_command(
            OUTPUT ${CMAKE_CURRENT_SOURCE_DIR}/cpac.h
            COMMAND ${CBINDGEN} --config cbindgen.toml --crate cpac-ffi --output cpac.h
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
            DEPENDS cpac-rust-build
            COMMENT "Generating cpac.h with cbindgen..."
        )
        add_custom_target(cpac-header ALL DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/cpac.h)
    else()
        message(WARNING "cbindgen not found, skipping header generation. Install with: cargo install cbindgen")
    endif()
endif()

# Import static library
if(CPAC_BUILD_STATIC)
    add_library(cpac-static STATIC IMPORTED GLOBAL)
    set_target_properties(cpac-static PROPERTIES
        IMPORTED_LOCATION "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${STATIC_LIB_NAME}"
    )
    add_dependencies(cpac-static cpac-rust-build)
    
    # Link dependencies (platform-specific)
    if(WIN32)
        target_link_libraries(cpac-static INTERFACE ws2_32 userenv bcrypt ntdll)
    elseif(APPLE)
        target_link_libraries(cpac-static INTERFACE "-framework Security" "-framework CoreFoundation")
    else()
        target_link_libraries(cpac-static INTERFACE dl pthread m)
    endif()
    
    # Install static library
    install(FILES "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${STATIC_LIB_NAME}"
            DESTINATION lib)
endif()

# Import shared library
if(CPAC_BUILD_SHARED)
    add_library(cpac-shared SHARED IMPORTED GLOBAL)
    if(WIN32)
        set_target_properties(cpac-shared PROPERTIES
            IMPORTED_LOCATION "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${SHARED_LIB_NAME}"
            IMPORTED_IMPLIB "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${SHARED_IMPLIB_NAME}"
        )
        install(FILES "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${SHARED_LIB_NAME}"
                DESTINATION bin)
    else()
        set_target_properties(cpac-shared PROPERTIES
            IMPORTED_LOCATION "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${SHARED_LIB_NAME}"
        )
        install(FILES "${CARGO_TARGET_DIR}/${RUST_BUILD_TYPE}/${SHARED_LIB_NAME}"
                DESTINATION lib)
    endif()
    add_dependencies(cpac-shared cpac-rust-build)
endif()

# Install header
install(FILES cpac.h DESTINATION include)

# Create alias targets for easier usage
if(CPAC_BUILD_STATIC)
    add_library(cpac::static ALIAS cpac-static)
endif()
if(CPAC_BUILD_SHARED)
    add_library(cpac::shared ALIAS cpac-shared)
endif()

# Export targets for find_package()
include(CMakePackageConfigHelpers)
write_basic_package_version_file(
    "${CMAKE_CURRENT_BINARY_DIR}/cpac-config-version.cmake"
    VERSION ${PROJECT_VERSION}
    COMPATIBILITY AnyNewerVersion
)

configure_package_config_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/cpac-config.cmake.in"
    "${CMAKE_CURRENT_BINARY_DIR}/cpac-config.cmake"
    INSTALL_DESTINATION lib/cmake/cpac
)

install(FILES
    "${CMAKE_CURRENT_BINARY_DIR}/cpac-config.cmake"
    "${CMAKE_CURRENT_BINARY_DIR}/cpac-config-version.cmake"
    DESTINATION lib/cmake/cpac
)
