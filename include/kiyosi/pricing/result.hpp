#pragma once

#include <array>
#include <cmath>
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

[[nodiscard]] constexpr std::optional<std::size_t> risk_measure_index(risk_measure measure) noexcept
{
    const auto index = static_cast<std::size_t>(measure);
    if (index >= risk_measure_count) return std::nullopt;
    return index;
}

struct PricingResult {
    using values_type = std::array<std::optional<double>, risk_measure_count>;

    PricingResult() = default;
    PricingResult(std::initializer_list<std::pair<risk_measure, double>> entries)
    {
        for (const auto& [measure, value] : entries)
            set(measure, value);
    }

    [[nodiscard]] bool has(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index && values_[*index].has_value();
    }

    [[nodiscard]] std::optional<double> get(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index ? values_[*index] : std::nullopt;
    }

    PricingResult& set(risk_measure measure, std::optional<double> value) noexcept
    {
        if (const auto index = risk_measure_index(measure)) values_[*index] = value;
        return *this;
    }

    PricingResult& set(risk_measure measure, double value) noexcept
    {
        return set(measure, std::optional<double>{value});
    }

    [[nodiscard]] const values_type& values_view() const noexcept { return values_; }

    [[nodiscard]] bool all_finite() const noexcept
    {
        for (const auto& value : values_)
            if (value && !std::isfinite(*value)) return false;
        return true;
    }

private:
    values_type values_{};
};

} // namespace kiyosi
