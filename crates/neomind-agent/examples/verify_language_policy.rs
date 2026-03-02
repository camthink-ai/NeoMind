use neomind_agent::prompts::builder::PromptBuilder;
use neomind_agent::translation::Language;

fn main() {
    let builder = PromptBuilder::new().with_language(Language::English);
    let prompt = builder.build_system_prompt();

    println!("=== First 800 characters of the prompt ===");
    println!("{}", &prompt[0..800.min(prompt.len())]);

    println!("\n\n=== Checking for language policy ===");
    println!("Contains 'CRITICAL LANGUAGE RULE': {}", prompt.contains("CRITICAL LANGUAGE RULE"));
    println!("Contains 'HIGHEST PRIORITY': {}", prompt.contains("HIGHEST PRIORITY"));
    println!("Contains 'EXACT SAME LANGUAGE': {}", prompt.contains("EXACT SAME LANGUAGE"));
}
