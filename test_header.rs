fn main() {
    use colored::Colorize;
    
    // Test various domain and provider counts
    for (domains, providers) in &[(1, 1), (3, 5), (10, 10), (100, 100), (1000, 1000)] {
        let dword = if *domains == 1 { "domain" } else { "domains" };
        let pword = if *providers == 1 { "provider" } else { "providers" };
        let rest = format!(" · scanning {domains} {dword} · {providers} {pword} ");
        
        let used = 2 + 3 + rest.chars().count();
        let pad = 58_usize.saturating_sub(used).max(3);
        
        let header = format!(
            "{}{}{}{}",
            "  ",
            "urx".truecolor(0x5a, 0xd1, 0xcd).bold(),
            rest.truecolor(0xa7, 0xb6, 0xc2),
            "─".repeat(pad).truecolor(0x5a, 0xd1, 0xcd).dimmed(),
        );
        
        // Strip ANSI
        let plain = console::strip_ansi_codes(&header);
        println!("Domains: {:4}, Providers: {:4} → plain len: {} (expected 58)", domains, providers, plain.chars().count());
    }
}
