#include "monte_carlo_sobol.hpp"

#include <array>
#include <bit>
#include <cassert>
#include <cstddef>
#include <cstdint>

#include <kiyosi/pricing/settings/monte_carlo.hpp>

#include "monte_carlo_random.hpp"

namespace kiyosi::detail {
namespace {

// Joe-Kuo new-joe-kuo-6.21201; source and BSD license are recorded in THIRD_PARTY_NOTICES.txt.
constexpr std::array<std::uint32_t, 397013> polynomial_data{
#include "sobol_joe_kuo.inc"
};

constexpr std::uint64_t sobol_mask = (std::uint64_t{1} << sobol_bits) - 1U;

std::uint64_t random_word(Pcg32& generator) noexcept
{
    return (static_cast<std::uint64_t>(generator()) << 32U) | generator();
}

} // namespace

SobolSequence::SobolSequence(std::size_t dimensions, std::uint64_t seed)
    : directions_(dimensions * sobol_bits), shifts_(dimensions)
{
    assert(dimensions > 0 && dimensions <= maximum_sobol_dimension);
    const auto* polynomial = polynomial_data.data();
    for (std::size_t dimension = 0; dimension < dimensions; ++dimension) {
        std::array<std::uint64_t, sobol_bits> raw{};
        if (dimension == 0) {
            for (unsigned int column = 0; column < sobol_bits; ++column)
                raw[column] = std::uint64_t{1} << (sobol_bits - column - 1U);
        } else {
            const auto degree = *polynomial++;
            const auto coefficients = *polynomial++;
            for (unsigned int column = 0; column < degree; ++column)
                raw[column] = static_cast<std::uint64_t>(*polynomial++) << (sobol_bits - column - 1U);
            for (unsigned int column = degree; column < sobol_bits; ++column) {
                auto direction = raw[column - degree] ^ (raw[column - degree] >> degree);
                for (unsigned int offset = 1; offset < degree; ++offset)
                    if (((coefficients >> (degree - offset - 1U)) & 1U) != 0)
                        direction ^= raw[column - offset];
                raw[column] = direction;
            }
        }

        Pcg32 generator{seed, static_cast<std::uint64_t>(dimension)};
        shifts_[dimension] = random_word(generator) & sobol_mask;
        std::array<std::uint64_t, sobol_bits> matrix{};
        for (unsigned int row = 0; row < sobol_bits; ++row) {
            const auto bit = sobol_bits - row - 1U;
            matrix[row] = (random_word(generator) & (sobol_mask << (bit + 1U))) |
                          (std::uint64_t{1} << bit);
        }
        for (unsigned int column = 0; column < sobol_bits; ++column) {
            std::uint64_t scrambled = 0;
            for (unsigned int row = 0; row < sobol_bits; ++row)
                scrambled |= static_cast<std::uint64_t>(std::popcount(raw[column] & matrix[row]) & 1) << (sobol_bits - row - 1U);
            directions_[static_cast<std::size_t>(column) * dimensions + dimension] = scrambled;
        }
    }
}

} // namespace kiyosi::detail
