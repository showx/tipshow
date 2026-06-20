package cmd

import (
	"fmt"
	"strings"

	"github.com/shengsheng/tipshow/internal/output"
	"github.com/shengsheng/tipshow/internal/tools"
	"github.com/spf13/cobra"
)

var doctorLevel string

var doctorCmd = &cobra.Command{
	Use:   "doctor",
	Short: "检测本机可用监控工具（由浅入深）",
	Run: func(cmd *cobra.Command, args []string) {
		rows := tools.AvailabilityReport()
		if doctorLevel != "" {
			filtered := rows[:0]
			for _, r := range rows {
				if r.Level == doctorLevel {
					filtered = append(filtered, r)
				}
			}
			rows = filtered
		}

		if output.JSONMode {
			if err := output.PrintJSON(rows); err != nil {
				fail(err)
			}
			return
		}

		var b strings.Builder
		current := ""
		for _, r := range rows {
			if r.Level != current {
				current = r.Level
				b.WriteString(fmt.Sprintf("\n[%s]\n", levelTitle(current)))
			}
			status := "✓"
			extra := r.Path
			if !r.Available {
				status = "✗"
				extra = r.InstallHint
			}
			b.WriteString(fmt.Sprintf("  %s %-10s %s\n", status, r.Name, r.Description))
			if extra != "" {
				b.WriteString(fmt.Sprintf("      → %s\n", extra))
			}
		}
		output.PrintText(strings.TrimSpace(b.String()))
	},
}

func levelTitle(level string) string {
	switch tools.Level(level) {
	case tools.LevelBasic:
		return "基础 basic"
	case tools.LevelIntermediate:
		return "中级 intermediate"
	case tools.LevelAdvanced:
		return "高级 advanced"
	default:
		return level
	}
}

func init() {
	doctorCmd.Flags().StringVar(&doctorLevel, "level", "", "只显示某层级: basic|intermediate|advanced")
}
