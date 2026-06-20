package output

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"text/tabwriter"
)

var JSONMode bool

func PrintText(s string) {
	fmt.Print(s)
	if !strings.HasSuffix(s, "\n") {
		fmt.Println()
	}
}

func PrintJSON(v any) error {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(v)
}

func PrintTable(headers []string, rows [][]string) {
	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, strings.Join(headers, "\t"))
	for _, row := range rows {
		fmt.Fprintln(w, strings.Join(row, "\t"))
	}
	_ = w.Flush()
}

func Emit(v any, textFn func() string) error {
	if JSONMode {
		return PrintJSON(v)
	}
	PrintText(textFn())
	return nil
}
