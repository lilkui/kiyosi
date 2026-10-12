#pragma once

#include <array>
#include <cstddef>

namespace kiyosi::detail {

template <std::size_t count>
struct SpotStencil;

template <>
struct SpotStencil<2> {
    static constexpr std::array offsets{1.0, -1.0};
    static constexpr std::size_t up = 0, down = 1;
};

template <>
struct SpotStencil<3> {
    static constexpr std::array offsets{0.0, 1.0, -1.0};
    static constexpr std::size_t center = 0, up = 1, down = 2;
};

template <>
struct SpotStencil<5> : SpotStencil<3> {
    static constexpr std::array offsets{0.0, 1.0, -1.0, 2.0, -2.0};
    static constexpr std::size_t up_twice = 3, down_twice = 4;
};

} // namespace kiyosi::detail
