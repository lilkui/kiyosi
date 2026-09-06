#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <initializer_list>
#include <optional>
#include <utility>
#include <kiyosi/core/types.hpp>

namespace kiyosi {

enum class risk_measure : std::uint8_t {
    price,
    delta,
    gamma,
    speed,
    theta,
    charm,
    color,
    vega,
    vanna,
    zomma,
    rho,
    count,
};

inline constexpr std::size_t risk_measure_count = static_cast<std::size_t>(risk_measure::count);
using risk_measure_set = std::uint16_t;

[[nodiscard]] constexpr std::optional<std::size_t> risk_measure_index(risk_measure measure) noexcept
{
    const auto index = static_cast<std::size_t>(measure);
    if (index >= risk_measure_count) return std::nullopt;
    return index;
}

[[nodiscard]] constexpr risk_measure_set risk_bit(risk_measure measure) noexcept
{
    const auto index = risk_measure_index(measure);
    return index ? static_cast<risk_measure_set>(risk_measure_set{1} << *index) : 0;
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure left, risk_measure right) noexcept
{
    return risk_bit(left) | risk_bit(right);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure_set left, risk_measure right) noexcept
{
    return left | risk_bit(right);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure left, risk_measure_set right) noexcept
{
    return risk_bit(left) | right;
}

inline constexpr risk_measure_set all_risk_measures =
    static_cast<risk_measure_set>((risk_measure_set{1} << risk_measure_count) - 1);

struct PricingResult {
    using values_type = std::array<std::optional<double>, risk_measure_count>;

    values_type values{};

    PricingResult() = default;
    PricingResult(std::initializer_list<std::pair<risk_measure, double>> entries)
    {
        for (const auto& [measure, value] : entries)
            set(measure, value);
    }

    [[nodiscard]] bool has(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index && values[*index].has_value();
    }

    [[nodiscard]] std::optional<double> get(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index ? values[*index] : std::nullopt;
    }

    PricingResult& set(risk_measure measure, std::optional<double> value) noexcept
    {
        if (const auto index = risk_measure_index(measure)) values[*index] = value;
        return *this;
    }

    PricingResult& set(risk_measure measure, double value) noexcept
    {
        return set(measure, std::optional<double>{value});
    }
};

struct PricingRequest {
    risk_measure_set measures = 0;

    [[nodiscard]] static constexpr PricingRequest price_only() noexcept
    {
        return PricingRequest{risk_bit(risk_measure::price)};
    }
    [[nodiscard]] static constexpr PricingRequest all() noexcept { return PricingRequest{}; }
    [[nodiscard]] constexpr bool requests(risk_measure measure) const noexcept
    {
        return measures == 0 ? risk_bit(measure) != 0 : (measures & risk_bit(measure)) != 0;
    }
};

} // namespace kiyosi
