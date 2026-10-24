#pragma once

#include <array>
#include <cassert>
#include <cmath>
#include <cstddef>
#include <numbers>

namespace kiyosi::detail {

// Cephes ndtri, Copyright 1984, 1987, 1989 Stephen L. Moshier;
// C++ translation Copyright 2024 SciPy. BSD notices are in THIRD_PARTY_NOTICES.txt.
template <std::size_t count>
inline double normal_polynomial(double value, const std::array<double, count>& coefficients,
                                double leading = 0.0) noexcept
{
    for (const double coefficient : coefficients)
        leading = leading * value + coefficient;
    return leading;
}

// Internal precondition: the Sobol midpoint conversion supplies a finite value in (0, 1).
inline double inverse_normal_cdf(double probability) noexcept
{
    assert(probability > 0.0 && probability < 1.0);
    constexpr std::array p0{
        -5.99633501014107895267E1,
        9.80010754185999661536E1,
        -5.66762857469070293439E1,
        1.39312609387279679503E1,
        -1.23916583867381258016E0,
    };
    constexpr std::array q0{
        1.95448858338141759834E0,
        4.67627912898881538453E0,
        8.63602421390890590575E1,
        -2.25462687854119370527E2,
        2.00260212380060660359E2,
        -8.20372256168333339912E1,
        1.59056225126211695515E1,
        -1.18331621121330003142E0,
    };
    constexpr std::array p1{
        4.05544892305962419923E0,
        3.15251094599893866154E1,
        5.71628192246421288162E1,
        4.40805073893200834700E1,
        1.46849561928858024014E1,
        2.18663306850790267539E0,
        -1.40256079171354495875E-1,
        -3.50424626827848203418E-2,
        -8.57456785154685413611E-4,
    };
    constexpr std::array q1{
        1.57799883256466749731E1,
        4.53907635128879210584E1,
        4.13172038254672030440E1,
        1.50425385692907503408E1,
        2.50464946208309415979E0,
        -1.42182922854787788574E-1,
        -3.80806407691578277194E-2,
        -9.33259480895457427372E-4,
    };
    constexpr std::array p2{
        3.23774891776946035970E0,
        6.91522889068984211695E0,
        3.93881025292474443415E0,
        1.33303460815807542389E0,
        2.01485389549179081538E-1,
        1.23716634817820021358E-2,
        3.01581553508235416007E-4,
        2.65806974686737550832E-6,
        6.23974539184983293730E-9,
    };
    constexpr std::array q2{
        6.02427039364742014255E0,
        3.67983563856160859403E0,
        1.37702099489081330271E0,
        2.16236993594496635890E-1,
        1.34204006088543189037E-2,
        3.28014464682127739104E-4,
        2.89247864745380683936E-6,
        6.79019408009981274425E-9,
    };
    constexpr double tail_boundary = 0.13533528323661269189;
    const bool upper = probability > 1.0 - tail_boundary;
    const double tail = upper ? 1.0 - probability : probability;
    if (tail > tail_boundary) {
        const double centered = tail - 0.5;
        const double square = centered * centered;
        return (centered + centered * square * normal_polynomial(square, p0) /
                               normal_polynomial(square, q0, 1.0)) *
               std::sqrt(2.0 * std::numbers::pi);
    }
    const double root = std::sqrt(-2.0 * std::log(tail));
    const double inverse = 1.0 / root;
    const double correction = inverse * (root < 8.0
                                             ? normal_polynomial(inverse, p1) / normal_polynomial(inverse, q1, 1.0)
                                             : normal_polynomial(inverse, p2) / normal_polynomial(inverse, q2, 1.0));
    const double quantile = root - std::log(root) / root - correction;
    return upper ? quantile : -quantile;
}

} // namespace kiyosi::detail
