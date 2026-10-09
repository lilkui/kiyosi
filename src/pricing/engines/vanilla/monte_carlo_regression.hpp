#pragma once

#include <algorithm>
#include <array>
#include <cmath>

namespace kiyosi::detail {

using QuadraticRegressionMatrix = std::array<std::array<double, 4>, 3>;

inline bool solve_quadratic(const QuadraticRegressionMatrix& samples,
                            std::array<double, 3>& coefficients)
{
    // Reduce the basis when paths cannot identify a quadratic continuation value.
    for (int degree = 2; degree >= 0; --degree) {
        auto matrix = samples;
        double scale = 0.0;
        for (int row = 0; row <= degree; ++row)
            for (int column = 0; column <= degree; ++column)
                scale = std::max(scale, std::abs(matrix[row][column]));
        bool singular = false;
        for (int column = 0; column <= degree; ++column) {
            int pivot = column;
            for (int row = column + 1; row <= degree; ++row)
                if (std::abs(matrix[row][column]) > std::abs(matrix[pivot][column])) pivot = row;
            if (!std::isfinite(matrix[pivot][column])) return false;
            if (std::abs(matrix[pivot][column]) <= 1e-14 * scale) {
                singular = true;
                break;
            }
            std::swap(matrix[column], matrix[pivot]);
            for (int row = column + 1; row <= degree; ++row) {
                const double factor = matrix[row][column] / matrix[column][column];
                for (int entry = column; entry <= degree; ++entry)
                    matrix[row][entry] -= factor * matrix[column][entry];
                matrix[row][3] -= factor * matrix[column][3];
            }
        }
        if (singular) continue;
        coefficients.fill(0.0);
        for (int row = degree; row >= 0; --row) {
            double value = matrix[row][3];
            for (int column = row + 1; column <= degree; ++column)
                value -= matrix[row][column] * coefficients[column];
            coefficients[row] = value / matrix[row][row];
        }
        return std::ranges::all_of(coefficients,
                                   [](double value) { return std::isfinite(value); });
    }
    return false;
}

} // namespace kiyosi::detail
