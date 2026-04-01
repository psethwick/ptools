import { Form, ActionPanel, Action } from "@raycast/api";

export default function Command() {
  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Submit"
            onSubmit={(values: Record<string, string>) => {
              console.log("Form submitted:", JSON.stringify(values));
            }}
          />
        </ActionPanel>
      }
    >
      <Form.TextField id="name" title="Name" placeholder="Enter your name" />
      <Form.Checkbox id="agree" title="Terms" label="I agree to the terms" />
      <Form.Dropdown id="color" title="Favorite color" defaultValue="blue">
        <Form.Dropdown.Item value="red" title="Red" />
        <Form.Dropdown.Item value="green" title="Green" />
        <Form.Dropdown.Item value="blue" title="Blue" />
      </Form.Dropdown>
    </Form>
  );
}
